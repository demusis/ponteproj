package io.github.demusis.ponte

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.background
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test

/** Pictures of the categories: task rows, the task screen, the picker, the filter and the document rows. */
class CategoriasTelasTest {
    @get:Rule val paparazzi = newPaparazzi()

    private val urgente = Category("u1", "Urgente", 4)
    private val juridico = Category("u2", "Jurídico", 1)
    private val financeiro = Category("u3", "Financeiro", 8)
    private val viagem = Category("u4", "Viagem de trabalho", 5)
    private val antiga = Category("u5", "Campanha antiga", 9, archived = true)
    private val library = listOf(urgente, juridico, financeiro, viagem, antiga)

    private fun withCategories(content: @Composable () -> Unit): @Composable () -> Unit = {
        CompositionLocalProvider(LocalCategories provides library) { content() }
    }

    private val tasks = Samples.tasks.mapIndexed { i, t ->
        when (i) {
            0 -> t.copy(categories = listOf("u1", "u2"))
            1 -> t.copy(categories = listOf("u3"))
            2 -> t.copy(categories = listOf("u2", "u3", "u4", "u1"))
            4 -> t.copy(categories = listOf("u4"))
            else -> t
        }
    }

    @Test fun tarefasComCategorias() {
        paparazzi.tall(3000)
        paparazzi.shotInShell("claro", 1) { withCategories { TasksScreen(TasksState(false, null, tasks), {}, {}) }() }
    }

    @Test fun tarefasComCategoriasEscuro() {
        paparazzi.tall(3000)
        paparazzi.shotInShell("escuro", 1, dark = true) { withCategories { TasksScreen(TasksState(false, null, tasks), {}, {}) }() }
    }

    @Test fun tarefasFiltradas() = paparazzi.shotInShell("filtrada", 1) {
        withCategories {
            TasksScreen(TasksState(false, null, tasks), {}, {}, categoryFilter = setOf("u2", CategoryRules.NONE))
        }()
    }

    @Test fun tarefasFiltroSemResultado() = paparazzi.shotInShell("sem_resultado", 1) {
        withCategories {
            TasksScreen(TasksState(false, null, tasks), {}, {}, categoryFilter = setOf("u5"))
        }()
    }

    private fun taskScreen(task: TaskDetail) = withCategories {
        TaskScreen(
            task.id, TaskEditState(id = task.id, loading = false, task = task), Samples.actions, Samples.priorities,
            {}, {}, {}, {}, {}, {}, {}
        )
    }

    @Test fun tarefaComCategorias() {
        paparazzi.tall(3900)
        val t = Samples.taskLinked.let { it.copy(form = it.form.copy(categories = listOf("u1", "u2", "u4"))) }
        paparazzi.shot("claro") { taskScreen(t)() }
    }

    @Test fun tarefaComCategoriasEscuro() {
        paparazzi.tall(3900)
        val t = Samples.taskLinked.let { it.copy(form = it.form.copy(categories = listOf("u1", "u2", "u4"))) }
        paparazzi.shot("escuro", dark = true) { taskScreen(t)() }
    }

    @Test fun tarefaSemCategorias() {
        paparazzi.tall(3900)
        paparazzi.shot("sem") { taskScreen(Samples.taskLinked)() }
    }

    @Test fun novaTarefaComCategorias() {
        paparazzi.tall(2600)
        paparazzi.shot("nova") {
            withCategories { NewTaskScreen("", Samples.actions, {}, { _, _, _, _, _, _, _ -> }, withoutDocument = true) }()
        }
    }

    // ---- the picker (its body, as it is inside the sheet)

    private fun sheet(content: @Composable () -> Unit): @Composable () -> Unit = {
        Box(Modifier.fillMaxWidth().background(Ponte.colors.surface).padding(top = 12.dp)) { content() }
    }

    private fun picker(selected: List<String>, query: String, error: String = "") = sheet {
        CategoryPickerContent(library, selected, query, {}, {}, {}, busy = false, error = error, onDone = {})
    }

    @Test fun seletor() {
        paparazzi.tall(1500)
        paparazzi.shot("claro") { picker(listOf("u1", "u4"), "")() }
    }

    @Test fun seletorEscuro() {
        paparazzi.tall(1500)
        paparazzi.shot("escuro", dark = true) { picker(listOf("u1", "u4"), "")() }
    }

    @Test fun seletorCriandoNaHora() {
        paparazzi.tall(1100)
        paparazzi.shot("criar") { picker(listOf("u1"), "Cartório")() }
    }

    @Test fun seletorNomeJaExiste() {
        paparazzi.tall(1100)
        paparazzi.shot("ja_existe") { picker(emptyList(), "juridico")() }
    }

    @Test fun seletorComArquivadaNoItem() {
        paparazzi.tall(1500)
        paparazzi.shot("arquivada") { picker(listOf("u5", "u2"), "")() }
    }

    @Test fun seletorSemCategorias() {
        paparazzi.tall(900)
        paparazzi.shot("vazio") {
            sheet { CategoryPickerContent(emptyList(), emptyList(), "", {}, {}, {}, busy = false, error = "", onDone = {}) }()
        }
    }

    @Test fun seletorComErro() {
        paparazzi.tall(1300)
        paparazzi.shot("erro") { picker(listOf("u1"), "Cartório", error = "Já existe uma categoria com esse nome.")() }
    }

    @Test fun filtro() {
        paparazzi.tall(1500)
        paparazzi.shot("claro") {
            sheet { CategoryFilterContent(library, setOf("u2", CategoryRules.NONE), {}, {}, {}) }()
        }
    }

    // ---- documents

    private val docs = Samples.docs.mapIndexed { i, d ->
        when (i) {
            0 -> d.copy(categories = listOf("u1", "u2", "u3"))
            1 -> d.copy(categories = listOf("u4"))
            else -> d
        }
    }

    @Test fun documentosComCategorias() {
        paparazzi.tall(2400)
        paparazzi.shotInShell("claro", 2) {
            withCategories { DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}) }()
        }
    }

    @Test fun documentosComCategoriasEscuro() {
        paparazzi.tall(2400)
        paparazzi.shotInShell("escuro", 2, dark = true) {
            withCategories { DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}) }()
        }
    }

    @Test fun documentosFiltrados() = paparazzi.shotInShell("filtrada", 2) {
        withCategories {
            DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}, categoryFilter = setOf("u4"))
        }()
    }

    @Test fun fichaComCategorias() {
        paparazzi.tall(3600)
        val d = Samples.detail.copy(categories = listOf("u1", "u2", "u3"), categoriesVersion = 5)
        paparazzi.shot("claro") {
            withCategories { DocumentScreen(DocumentState(false, null, d), false, {}, {}, {}, {}, {}, {}, {}) }()
        }
    }

    @Test fun fichaSemCategorias() {
        paparazzi.tall(3600)
        paparazzi.shot("sem") {
            withCategories { DocumentScreen(DocumentState(false, null, Samples.detail), false, {}, {}, {}, {}, {}, {}, {}) }()
        }
    }
}
