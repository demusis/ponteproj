package io.github.demusis.ponte

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import app.cash.paparazzi.DeviceConfig
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/**
 * The lean lists: the title as the area selector, the sync dot and when its text shows, the filter icon and its
 * summary line, the second line of a task row (rules), and the pictures of every list in the new frame.
 */
class ReformaListasTest {
    @get:Rule val paparazzi = newPaparazzi()

    // ---------------------------------------------------------------- rules

    private fun task(
        id: Long = 1, title: String = "Verificar possibilidades", action: String = "Examinar", number: String = "MANUAL-2026/0003",
        documentId: Long = 7, priority: String = "normal", done: Boolean = false
    ) = TaskItem(id, documentId, title, action, number, "", "", done, false, false, priority = priority)

    @Test fun oTituloDaAreaDizOndeEstaEQueSeTroca() {
        assertEquals("Tarefas", AreaSelector.title(TasksSegments.TASKS))
        assertEquals("Projetos", AreaSelector.title(TasksSegments.PROJECTS))
        assertEquals("Atendimentos", AreaSelector.title(TasksSegments.ATTENDANCES))
        assertEquals(PhoneWords.RES_AREA, AreaSelector.title(TasksSegments.RESOURCES))
        // An id nobody knows reads as the first area, never as an empty title.
        assertEquals("Tarefas", AreaSelector.title(99))
        assertEquals("Área: Projetos. Toque para trocar.", AreaSelector.description(TasksSegments.PROJECTS))
    }

    @Test fun oMenuDeAreasTemAsQuatroNaOrdemDaBarraAntiga() {
        assertEquals(
            listOf("Tarefas", "Projetos", "Atendimentos", PhoneWords.RES_AREA),
            AreaSelector.options().map { it.second }
        )
        assertEquals(TasksSegments.ORDER, AreaSelector.options().map { it.first })
        assertEquals("Projetos, área atual", AreaSelector.optionDescription(TasksSegments.PROJECTS, TasksSegments.PROJECTS))
        assertEquals("Projetos", AreaSelector.optionDescription(TasksSegments.PROJECTS, TasksSegments.TASKS))
    }

    @Test fun oTituloSegueAAreaEAsOutrasAbasTemTituloFixo() {
        val shell = ShellState()
        assertEquals("Hoje", AreaSelector.heading(shell.copy(tab = 0)))
        assertEquals("Tarefas", AreaSelector.heading(shell.copy(tab = 1)))
        assertEquals("Projetos", AreaSelector.heading(shell.copy(tab = 1, projects = true)))
        assertEquals("Atendimentos", AreaSelector.heading(shell.copy(tab = 1, attendances = true)))
        assertEquals(PhoneWords.RES_AREA, AreaSelector.heading(shell.copy(tab = 1, resources = true)))
        assertEquals("Documentos", AreaSelector.heading(shell.copy(tab = 2)))
        assertEquals("E-mail", AreaSelector.heading(shell.copy(tab = 3)))
        // Only Tarefas has areas to choose from.
        assertEquals(listOf(false, true, false, false), (0..3).map { AreaSelector.selectable(shell.copy(tab = it)) })
        // The flags of the frame map back to the controller's segment.
        assertEquals(TasksSegments.RESOURCES, AreaSelector.of(shell.copy(resources = true)))
        assertEquals(TasksSegments.TASKS, AreaSelector.of(shell))
    }

    @Test fun oTextoDaSincronizacaoSoApareceQuandoImporta() {
        val ok = ShellState(statusLine = "Sincronizado · 14:32", statusKind = StatusKind.OK)
        // Quiet: nothing to say.
        assertFalse(SyncDot.lineVisible(ok, recentlyEnded = false, peek = false))
        // A few seconds after a sync, and while the dot was tapped.
        assertTrue(SyncDot.lineVisible(ok, recentlyEnded = true, peek = false))
        assertTrue(SyncDot.lineVisible(ok, recentlyEnded = false, peek = true))
        // While it runs.
        assertTrue(SyncDot.lineVisible(ok.copy(syncing = true, statusLine = "Sincronizando…", statusKind = StatusKind.NEUTRAL), false, false))
        // Every state that is not "Sincronizado" stays in view: a problem never becomes invisible.
        for (kind in listOf(StatusKind.ERROR, StatusKind.WAITING, StatusKind.NEUTRAL)) {
            assertTrue(kind.name, SyncDot.lineVisible(ok.copy(statusKind = kind), false, false))
        }
    }

    @Test fun oPontoFalaOTextoTodoENaoSomenteACor() {
        val ok = ShellState(statusLine = "Sincronizado · 14:32", statusKind = StatusKind.OK)
        assertEquals("Sincronização: Sincronizado · 14:32", SyncDot.description(ok))
        assertEquals(
            "Sincronização com problema: Sem Wi-Fi: sincronização adiada.",
            SyncDot.description(ok.copy(statusLine = "Sem Wi-Fi: sincronização adiada.", statusKind = StatusKind.ERROR))
        )
        assertEquals(
            "Sincronização pendente: Alterações aguardando envio",
            SyncDot.description(ok.copy(statusLine = "Alterações aguardando envio", statusKind = StatusKind.WAITING))
        )
        assertEquals("Sincronizando…", SyncDot.description(ok.copy(syncing = true, statusLine = "Sincronizando…", statusKind = StatusKind.NEUTRAL)))
        // Nothing synced yet: the text says so even with an empty line.
        assertEquals("Ainda não sincronizado", SyncDot.text(ShellState()))
        assertEquals("Sincronização: Ainda não sincronizado", SyncDot.description(ShellState()))
    }

    @Test fun aLinhaDoFiltroDizOQueEstaLigado() {
        val library = listOf(Category("u1", "Estudo", 1), Category("u2", "Viagem", 2), Category("u3", "Casa", 3), Category("u4", "Antiga", 4, archived = true))
        val projects = listOf(ProjectItem("p1", "Projeto X", "", "", "ativo", "Ativo", "", "", 1, ProjectProgress()))
        assertEquals("", FilterSummary.line(emptyList(), ""))
        assertEquals("Filtro: Estudo", FilterSummary.line(listOf("Estudo"), ""))
        assertEquals("Filtro: Estudo · Projeto X", FilterSummary.line(listOf("Estudo"), "Projeto X"))
        assertEquals("Filtro: Projeto X", FilterSummary.line(emptyList(), "Projeto X"))
        assertEquals("Filtro: Estudo, Viagem +1", FilterSummary.line(listOf("Estudo", "Viagem", "Casa"), ""))
        // Names come from the library, "Sem categoria" first; an uid the library does not know is left out.
        assertEquals(listOf("Sem categoria", "Estudo"), FilterSummary.categoryNames(setOf("u1", CategoryRules.NONE), library))
        assertEquals(listOf("Estudo"), FilterSummary.categoryNames(setOf("u1", "fantasma"), library))
        assertEquals("", FilterSummary.projectName("", projects))
        assertEquals("Sem projeto", FilterSummary.projectName(ProjectRules.NONE, projects))
        assertEquals("Projeto X", FilterSummary.projectName("p1", projects))
    }

    @Test fun oIconeDoFiltroSoApareceComOQueFiltrarEDizSeHaFiltro() {
        val library = listOf(Category("u1", "Estudo", 1))
        val projects = listOf(ProjectItem("p1", "Projeto X", "", "", "ativo", "Ativo", "", "", 1, ProjectProgress()))
        assertTrue(FilterSummary.hasCategories(library, emptySet()))
        assertFalse(FilterSummary.hasCategories(emptyList(), emptySet()))
        // A chosen filter keeps its section even when the library no longer has anything (so it can be cleared).
        assertTrue(FilterSummary.hasCategories(emptyList(), setOf("u1")))
        assertTrue(FilterSummary.hasProjects(projects, ""))
        assertFalse(FilterSummary.hasProjects(emptyList(), ""))
        assertTrue(FilterSummary.hasProjects(emptyList(), ProjectRules.NONE))
        assertFalse(FilterSummary.isActive(emptySet(), ""))
        assertTrue(FilterSummary.isActive(setOf("u1"), ""))
        assertTrue(FilterSummary.isActive(emptySet(), "p1"))
        assertEquals("Filtros", FilterSummary.iconDescription(false))
        assertEquals("Filtros, há um filtro ativo", FilterSummary.iconDescription(true))
        // The icon belongs to the task list (Tarefas area) and to Documentos.
        val shell = ShellState()
        assertTrue(FilterIcon.forTasks(shell.copy(tab = 1)))
        assertFalse(FilterIcon.forTasks(shell.copy(tab = 1, projects = true)))
        assertFalse(FilterIcon.forTasks(shell.copy(tab = 0)))
        assertTrue(FilterIcon.forDocuments(shell.copy(tab = 2)))
        assertFalse(FilterIcon.forDocuments(shell.copy(tab = 1)))
    }

    @Test fun aSegundaLinhaDaTarefaNaoDizSemDocumento() {
        assertEquals("Examinar · MANUAL-2026/0003", TaskRowText.lead(task()))
        // No document: the kind alone, never "Sem documento".
        assertEquals("Examinar", TaskRowText.lead(task(documentId = 0, number = "")))
        assertEquals("MANUAL-2026/0003", TaskRowText.lead(task(action = "")))
        assertEquals("", TaskRowText.lead(task(action = "", documentId = 0, number = "")))
    }

    @Test fun soAPrioridadeAcimaDoNormalApareceNaLinha() {
        assertNull(TaskRowText.mark(task(priority = "normal")))
        assertNull(TaskRowText.mark(task(priority = "baixa")))
        assertEquals(TaskRowText.Mark("alta", "alta"), TaskRowText.mark(task(priority = "alta")))
        assertEquals(TaskRowText.Mark("urgente", "urgente"), TaskRowText.mark(task(priority = "urgente")))
        // A finished task shows no priority.
        assertNull(TaskRowText.mark(task(priority = "urgente", done = true)))
    }

    @Test fun asCategoriasQueCabemFicamEOrestoViraMais() {
        val w = listOf(60f, 50f, 70f)
        // Everything fits: no "+N" is needed, so none is reserved.
        assertEquals(3, TaskRowText.fitting(w, 200f, 0f, 20f))
        // The last one does not fit: two are shown and the room of "+1" was kept.
        assertEquals(2, TaskRowText.fitting(w, 150f, 0f, 20f))
        // Only the first fits with its "+N".
        assertEquals(1, TaskRowText.fitting(w, 90f, 0f, 20f))
        // Not even one fits with the "+N": none are shown.
        assertEquals(0, TaskRowText.fitting(w, 70f, 0f, 20f))
        assertEquals(0, TaskRowText.fitting(emptyList(), 100f, 0f, 20f))
        // A gap before each one counts.
        assertEquals(2, TaskRowText.fitting(w, 150f, 5f, 20f))
        assertEquals(1, TaskRowText.fitting(w, 140f, 5f, 20f))
    }

    @Test fun aLinhaDaTarefaParaOLeitorDeTelaTemTudo() {
        val t = task(priority = "alta", title = "Verificar").copy(dueChip = "há 2 dias", overdue = true, projectName = "Projeto X")
        assertEquals(
            "Verificar. atrasada, há 2 dias. Examinar · MANUAL-2026/0003. prioridade alta. categorias Estudo, Viagem. projeto Projeto X",
            TaskRowText.description(t, listOf("Estudo", "Viagem"), showProject = true)
        )
        assertEquals(
            "Verificar. atrasada, há 2 dias. Examinar · MANUAL-2026/0003. prioridade alta",
            TaskRowText.description(t, emptyList(), showProject = false)
        )
        assertEquals(
            "Verificar. concluída. Examinar · MANUAL-2026/0003",
            TaskRowText.description(task(title = "Verificar", done = true, priority = "alta"), emptyList(), showProject = true)
        )
    }

    @Test fun puxarParaAtualizarSoNasListasOndeSincronizarFazSentido() {
        assertEquals(listOf(true, true, true, false), (0..3).map { PullRules.enabled(it) })
        // The spinner gives up on a refused sync soon, and never holds for ever on one that does not end.
        assertTrue(PullRules.START_WAIT_MS in 500L..3000L)
        assertTrue(PullRules.MAX_SPIN_MS >= 60_000L)
    }

    // ---------------------------------------------------------------- pictures

    private val library = listOf(
        Category("u1", "Estudo", 1), Category("u2", "Jurídico", 4), Category("u3", "Financeiro", 8), Category("u4", "Viagem de trabalho", 5)
    )
    private val projetos = listOf(
        ProjectItem(
            "p1", "Mudança da sede para o novo prédio", "Levar todos os setores para o prédio novo.", "Maria Aparecida dos Santos Oliveira",
            "ativo", "Ativo", "2026-11-01", "2026-11-24", 7, ProjectProgress(8, 3, 2, "2026-11-10"), ProjectRisk(1, "2026-11-25T18:00")
        ),
        ProjectItem("p2", "Auditoria interna", "Revisar os contratos de 2026.", "Ana", "ativo", "Ativo", "", "2026-12-15", 2, ProjectProgress(5, 5, 0, "")),
        ProjectItem(
            "p3", "Revisão do plano de contratações 2027", "Consolidar as demandas dos setores.", "Servidor A", "ativo", "Ativo",
            "2026-11-03", "2026-12-20", 1, ProjectProgress(12, 4, 0, "2026-11-21"), ProjectRisk(3, "")
        ),
        ProjectItem("p4", "Campanha de vacinação", "", "", "pausado", "Pausado", "", "", 1, ProjectProgress())
    )

    /** The tasks of the owner's screen: a URL title, a priority, categories, a project, no document, late and today. */
    private val tarefas: List<TaskItem> = listOf(
        TaskItem(
            31, 131, "Responder ofício sobre o pessoal do Projeto Alfa", "Responder", "OF. 0412/2026", "2026-11-05", "há 4 dias",
            false, true, false, priority = "urgente", bucket = "atrasadas", categories = listOf("u2"),
            projectUid = "p1", projectName = "Mudança da sede para o novo prédio"
        ),
        TaskItem(32, 132, "Enviar relatório mensal de atividades", "Encaminhar", "MEM. 118/2026", "2026-11-07", "há 2 dias", false, true, false, bucket = "atrasadas"),
        TaskItem(
            33, 133, "Confirmar presença na reunião do Comitê Gestor", "Responder", "Convite reunião Comitê Gestor", "2026-11-21", "hoje",
            false, false, true, priority = "alta", bucket = "hoje", categories = listOf("u1", "u2", "u3")
        ),
        TaskItem(34, 134, "Conferir planilha de veículos", "Analisar", "PROC. 2026/5512", "2026-11-25", "25/11", false, false, false, bucket = "proximas"),
        TaskItem(
            35, 135, "https://github.com/anthropics/claude-code/issues/12345678901234567890123456789", "Examinar", "MANUAL-2026/0003", "", "",
            false, false, false, priority = "alta", bucket = "sem_prazo", categories = listOf("u1")
        ),
        TaskItem(36, 0, "Ligar para o setor de protocolo", "Acompanhar", "", "", "", false, false, false, bucket = "sem_prazo", categories = listOf("u3", "u4", "u1", "u2")),
        TaskItem(37, 137, "Verificar possibilidades", "Examinar", "MANUAL-2026/0003", "", "", false, false, false, bucket = "sem_prazo"),
        TaskItem(38, 138, "Assinar termo de recebimento", "Assinar", "TERMO 22/2026", "2026-11-03", "03/11", true, false, false, bucket = "concluidas")
    )

    private fun withLibrary(content: @Composable () -> Unit): @Composable () -> Unit = {
        CompositionLocalProvider(LocalCategories provides library) { content() }
    }

    private val inactive = ShellFilter(false) {}
    private val active = ShellFilter(true) {}

    private fun phone(font: Float = 1f) {
        paparazzi.unsafeUpdateConfig(deviceConfig = DeviceConfig.PIXEL_5.copy(fontScale = font))
    }

    /** 320 dp wide (the narrowest phone), 640 px at 320 dpi. */
    private fun narrow(font: Float = 1f, heightPx: Int = 1500) {
        paparazzi.unsafeUpdateConfig(
            deviceConfig = DeviceConfig.PIXEL_5.copy(
                screenWidth = 640, screenHeight = heightPx, xdpi = 320, ydpi = 320,
                density = com.android.resources.Density.XHIGH, fontScale = font
            )
        )
    }

    private val tasksScreen = withLibrary {
        TasksScreen(TasksState(false, null, tarefas), {}, {}, projects = projetos)
    }

    @Test fun tarefas() = paparazzi.shotInShell("claro", 1, filter = inactive, content = tasksScreen)
    @Test fun tarefasEscuro() = paparazzi.shotInShell("escuro", 1, dark = true, filter = inactive, content = tasksScreen)

    @Test fun tarefasInteira() {
        paparazzi.tall(3000)
        paparazzi.shotInShell("inteira", 1, filter = inactive, content = tasksScreen)
    }

    @Test fun tarefasLetra13() {
        phone(font = 1.3f)
        paparazzi.shotInShell("letra_1_3", 1, filter = inactive, content = tasksScreen)
    }

    @Test fun tarefasLetra13Escuro() {
        phone(font = 1.3f)
        paparazzi.shotInShell("letra_1_3_escuro", 1, dark = true, filter = inactive, content = tasksScreen)
    }

    @Test fun tarefas320() {
        narrow()
        paparazzi.shotInShell("320", 1, filter = inactive, content = tasksScreen)
    }

    @Test fun tarefas320Letra13() {
        narrow(1.3f, 1700)
        paparazzi.shotInShell("320_letra_1_3", 1, dark = true, filter = inactive, content = tasksScreen)
    }

    @Test fun tarefasComFiltroAtivo() = paparazzi.shotInShell("filtro_ativo", 1, filter = active) {
        withLibrary {
            TasksScreen(
                TasksState(false, null, tarefas), {}, {}, projects = projetos,
                categoryFilter = setOf("u1", "u2")
            )
        }()
    }

    @Test fun tarefasComFiltroAtivoEscuro() = paparazzi.shotInShell("filtro_ativo_escuro", 1, dark = true, filter = active) {
        withLibrary {
            TasksScreen(TasksState(false, null, tarefas), {}, {}, projects = projetos, categoryFilter = setOf("u2"), projectFilter = "p1")
        }()
    }

    @Test fun tarefasFiltroSemResultado() = paparazzi.shotInShell("filtro_sem_resultado", 1, filter = active) {
        withLibrary {
            TasksScreen(TasksState(false, null, tarefas), {}, {}, projects = projetos, categoryFilter = setOf("u4"))
        }()
    }

    // The states of the sync in the frame.
    private val lineError = Samples.shell.copy(
        statusLine = "Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes.", statusKind = StatusKind.ERROR
    )

    @Test fun sincronizacaoComErro() = paparazzi.shotInShell("erro", 1, shell = lineError, filter = inactive, content = tasksScreen)

    @Test fun sincronizacaoComErroEscuro() =
        paparazzi.shotInShell("erro_escuro", 1, dark = true, shell = lineError, filter = inactive, content = tasksScreen)

    @Test fun sincronizacaoPendente() = paparazzi.shotInShell(
        "pendente", 1, shell = Samples.shell.copy(statusLine = "Alterações aguardando envio", statusKind = StatusKind.WAITING),
        filter = inactive, content = tasksScreen
    )

    @Test fun sincronizando() = paparazzi.shotInShell(
        "sincronizando", 1, shell = Samples.shell.copy(syncing = true, statusLine = "Sincronizando…", statusKind = StatusKind.NEUTRAL),
        filter = inactive, content = tasksScreen
    )

    @Test fun sincronizadoSemTexto() = paparazzi.shotInShell("so_o_ponto", 1, filter = inactive, content = tasksScreen)

    // The other areas of the Tarefas tab, and the other tabs.
    @Test fun projetos() = paparazzi.shotInShell("claro", 1, shell = Samples.shell.copy(projects = true)) {
        ProjectsScreen(ProjectsState(false, null, projetos), "ativo", {}, "", {}, {}, {})
    }

    @Test fun projetosEscuro() = paparazzi.shotInShell("escuro", 1, dark = true, shell = Samples.shell.copy(projects = true)) {
        ProjectsScreen(ProjectsState(false, null, projetos), "ativo", {}, "", {}, {}, {})
    }

    @Test fun atendimentos() = paparazzi.shotInShell("claro", 1, shell = Samples.shell.copy(attendances = true)) {
        AttendancesScreen(MoreSamples.attendancesState, "", {}, {})
    }

    @Test fun atendimentosEscuro() = paparazzi.shotInShell("escuro", 1, dark = true, shell = Samples.shell.copy(attendances = true)) {
        AttendancesScreen(MoreSamples.attendancesState, "", {}, {})
    }

    @Test fun atendimentos320Letra13() {
        narrow(1.3f, 1700)
        paparazzi.shotInShell("320_letra_1_3", 1, shell = Samples.shell.copy(attendances = true)) {
            AttendancesScreen(MoreSamples.attendancesState, "", {}, {})
        }
    }

    @Test fun recursosUsos() = paparazzi.shotInShell("usos_claro", 1, shell = Samples.shell.copy(resources = true)) {
        ResourcesScreen(ResourceSamples.state, "", {}, 0, {}, {}, {})
    }

    @Test fun recursosUsosEscuro() = paparazzi.shotInShell("usos_escuro", 1, dark = true, shell = Samples.shell.copy(resources = true)) {
        ResourcesScreen(ResourceSamples.state, "", {}, 0, {}, {}, {})
    }

    @Test fun recursosCadastro() = paparazzi.shotInShell("cadastro_claro", 1, shell = Samples.shell.copy(resources = true)) {
        ResourcesScreen(ResourceSamples.state, "", {}, 1, {}, {}, {})
    }

    @Test fun hoje() {
        paparazzi.tall(3900)
        paparazzi.shotInShell("claro", 0, shell = Samples.shell.copy(badge = 4), content = withLibrary { TodayScreen(Samples.today, {}, {}) })
    }

    @Test fun hojeEscuro() {
        paparazzi.tall(3900)
        paparazzi.shotInShell("escuro", 0, dark = true, shell = Samples.shell.copy(badge = 4), content = withLibrary { TodayScreen(Samples.today, {}, {}) })
    }

    private val docs = Samples.docs.mapIndexed { i, d ->
        when (i) {
            0 -> d.copy(categories = listOf("u1", "u2"))
            1 -> d.copy(categories = listOf("u3"))
            else -> d
        }
    }

    @Test fun documentos() {
        paparazzi.tall(2400)
        paparazzi.shotInShell("claro", 2, filter = inactive, content = withLibrary { DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}) })
    }

    @Test fun documentosEscuro() {
        paparazzi.tall(2400)
        paparazzi.shotInShell("escuro", 2, dark = true, filter = inactive, content = withLibrary { DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}) })
    }

    @Test fun documentosComFiltro() = paparazzi.shotInShell("filtro_ativo", 2, filter = active) {
        withLibrary { DocumentsScreen(docs, "", {}, DocFilter.ALL, {}, {}, {}, categoryFilter = setOf("u1")) }()
    }

    // What a pop-up draws, drawn in place (the pictures cannot open one).
    @Composable
    private fun MenuCard(modifier: Modifier, content: @Composable ColumnScope.() -> Unit) {
        Surface(
            modifier, shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.surfaceContainer, shadowElevation = 6.dp
        ) { Column(Modifier.width(IntrinsicSize.Max).widthIn(min = 200.dp).padding(vertical = 8.dp), content = content) }
    }

    private fun areaMenu(dark: Boolean, name: String, area: Int) = paparazzi.shotInShell(name, 1, dark = dark, shell = Samples.shell.copy(
        projects = area == TasksSegments.PROJECTS, attendances = area == TasksSegments.ATTENDANCES, resources = area == TasksSegments.RESOURCES
    ), filter = inactive) {
        Box(Modifier.fillMaxSize()) {
            withLibrary { TasksScreen(TasksState(false, null, tarefas), {}, {}, projects = projetos) }()
            MenuCard(Modifier.padding(start = 8.dp, top = 4.dp)) { AreaMenuItems(area) {} }
        }
    }

    @Test fun menuDeAreas() = areaMenu(false, "menu_areas", TasksSegments.TASKS)
    @Test fun menuDeAreasEscuro() = areaMenu(true, "menu_areas_escuro", TasksSegments.PROJECTS)

    @Test fun menuDeMais() = paparazzi.shotInShell("menu_mais", 1, filter = inactive) {
        Box(Modifier.fillMaxSize()) {
            tasksScreen()
            MenuCard(Modifier.align(Alignment.TopEnd).padding(end = 8.dp, top = 4.dp)) {
                DropdownMenuItem(
                    text = { Text("Sincronizar agora") }, onClick = {},
                    leadingIcon = { Icon(Icons.Outlined.Sync, null) }
                )
                DropdownMenuItem(
                    text = { Text("Ajustes") }, onClick = {},
                    leadingIcon = { Icon(Icons.Outlined.Tune, null) }
                )
            }
        }
    }

    private fun sheet(dark: Boolean, name: String, cats: Set<String>, project: String) =
        paparazzi.shotInShell(name, 1, dark = dark, filter = ShellFilter(cats.isNotEmpty() || project.isNotEmpty()) {}) {
            Box(Modifier.fillMaxSize()) {
                withLibrary { TasksScreen(TasksState(false, null, tarefas), {}, {}, projects = projetos, categoryFilter = cats, projectFilter = project) }()
                Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.32f)))
                Surface(
                    Modifier.align(Alignment.BottomCenter), color = Ponte.colors.surface,
                    shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)
                ) {
                    FilterSheetContent(library, cats, {}, projetos, project, {}, {}, {})
                }
            }
        }

    @Test fun folhaDeFiltros() = sheet(false, "folha", emptySet(), "")
    @Test fun folhaDeFiltrosEscuroComEscolha() = sheet(true, "folha_escuro", setOf("u1", "u3"), "p3")
}
