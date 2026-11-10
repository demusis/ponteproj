package io.github.demusis.ponte

import org.junit.Rule
import org.junit.Test

/** Pictures of Hoje, of the task screen and of the document picker. */
class HojeTarefaTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val busyShell = Samples.shell.copy(badge = 4)

    @Test fun hoje() {
        paparazzi.tall(3900)
        paparazzi.shotInShell("claro", 0, shell = busyShell) { TodayScreen(Samples.today, {}, {}) }
    }

    @Test fun hojeEscuro() {
        paparazzi.tall(3900)
        paparazzi.shotInShell("escuro", 0, dark = true, shell = busyShell) { TodayScreen(Samples.today, {}, {}) }
    }

    @Test fun hojeVazio() = paparazzi.shotInShell("vazio", 0, shell = Samples.shell.copy(badge = 0)) {
        TodayScreen(TodayState(loading = false), {}, {})
    }

    @Test fun hojeVazioEscuro() = paparazzi.shotInShell("vazio_escuro", 0, dark = true, shell = Samples.shell.copy(badge = 0)) {
        TodayScreen(TodayState(loading = false), {}, {})
    }

    @Test fun hojeSoProximos() = paparazzi.shotInShell("so_proximos", 0, shell = Samples.shell.copy(badge = 0)) {
        TodayScreen(TodayState(loading = false, upcoming = Samples.todayUpcoming), {}, {})
    }

    private fun screen(
        task: TaskDetail, state: TaskEditState = TaskEditState(loading = false, task = task), me: String = "Celular (Pixel 5)"
    ): @androidx.compose.runtime.Composable () -> Unit = {
        TaskScreen(state, Samples.actions, Samples.priorities, me, {}, {}, {}, {}, {}, {})
    }

    @Test fun tarefaComDocumento() {
        paparazzi.tall(3700)
        paparazzi.shot("claro") { screen(Samples.taskLinked)() }
    }

    @Test fun tarefaComDocumentoEscuro() {
        paparazzi.tall(3700)
        paparazzi.shot("escuro", dark = true) { screen(Samples.taskLinked)() }
    }

    @Test fun tarefaSemDocumento() {
        paparazzi.tall(3300)
        paparazzi.shot("claro") { screen(Samples.taskLoose)() }
    }

    @Test fun tarefaSemDocumentoEscuro() {
        paparazzi.tall(3300)
        paparazzi.shot("escuro", dark = true) { screen(Samples.taskLoose)() }
    }

    @Test fun tarefaComErro() {
        paparazzi.tall(3700)
        paparazzi.shot("conflito") {
            screen(
                Samples.taskLinked,
                TaskEditState(
                    loading = false, task = Samples.taskLinked,
                    formError = "Esta tarefa foi alterada em outro aparelho. Abra-a de novo para ver a versão atual."
                )
            )()
        }
    }

    @Test fun tarefaCarregando() = paparazzi.shot("carregando") { screen(Samples.taskLinked, TaskEditState())() }

    @Test fun escolherDocumento() = paparazzi.shot("claro") {
        DocumentPickerScreen(Samples.docs, 0, false, {}, {})
    }

    @Test fun escolherDocumentoEscuro() = paparazzi.shot("escuro", dark = true) {
        DocumentPickerScreen(Samples.docs, 101, false, {}, {})
    }

    @Test fun novaTarefaSemDocumento() = paparazzi.shot("claro") {
        NewTaskScreen("", Samples.actions.filter { it.key != "triar" }, {}, { _, _, _ -> }, withoutDocument = true)
    }

    @Test fun tarefasComBotao() = paparazzi.shotInShell("com_botao", 1) { TasksScreen(TasksState(false, null, Samples.tasks), {}, {}) }
}
