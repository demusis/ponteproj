package io.github.demusis.ponte

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.ui.Modifier
import org.junit.Rule
import org.junit.Test

/** Sample data of the atendimentos, the search and the person's name (all generic). */
internal object MoreSamples {
    private fun att(
        id: Long, ref: String, subject: String, status: String, due: String, chip: String,
        late: Boolean = false, today: Boolean = false, open: Boolean = true, assignee: String = ""
    ) = AttendanceItem(id, ref, subject, status, due, chip, late, assignee, "", open, today)

    val attendances = listOf(
        att(1, "ORG-OFI-2026/00431", "Pedido de apoio logístico para evento", "Em análise", "2026-11-06", "há 4 dias", late = true, assignee = "Servidor A"),
        att(2, "ORG-PRO-2026/00212", "Consulta sobre o quadro de pessoal", "Aguardando resposta", "2026-11-10", "hoje", today = true, assignee = "Servidor B"),
        att(3, "ORG-OFI-2026/00500", "Solicitação de relatório trimestral", "Em elaboração", "2026-11-13", "13/11", assignee = "Servidor A"),
        att(4, "ORG-MEM-2026/00077", "Memorando sobre a escala de trabalho", "A distribuir", "", ""),
        att(5, "ORG-OFI-2026/00100", "Resposta ao pedido de informação", "Encerrado", "2026-10-20", "20/10", open = false, assignee = "Servidor A"),
        att(6, "ORG-OFI-2026/00101", "Convite para reunião", "Cancelado", "", "", open = false)
    )

    val attendancesState = AttendancesState(loading = false, rows = attendances)

    val ficha = AttendanceDetail(
        id = 1, reference = "ORG-OFI-2026/00431", subject = "Pedido de apoio logístico para evento",
        statusLabel = "Em análise", open = true, dueLabel = "06/11/2026", late = true, dueToday = false,
        fields = listOf(
            AttendanceField("Situação", "Em análise"),
            AttendanceField("Prioridade", "Alta"),
            AttendanceField("Motivo da prioridade", "Evento marcado para a próxima semana"),
            AttendanceField("Prazo", "06/11/2026"),
            AttendanceField("Tipo de prazo", "Prazo oficial"),
            AttendanceField("Responsável", "Servidor A"),
            AttendanceField("Entrada", "27/10/2026"),
            AttendanceField("Recebido de", "Servidor B"),
            AttendanceField("Observações", "Pedir à coordenação a lista de participantes antes de responder."),
            AttendanceField("Retorno para UAS?", "Sim"),
            AttendanceField("Enviado para", "Setor Jurídico")
        ),
        flags = listOf("Documento sensível"),
        reviewFlags = listOf("Prazo textual pendente de confirmação"),
        documentId = 101, documentNumber = "ORG-OFI-2026/00431", documentTitle = "Solicitação de apoio logístico",
        tasks = listOf(
            AttendanceTask(1, "Responder ofício sobre o apoio logístico", false, "12/11/2026"),
            AttendanceTask(8, "Conferir a lista de participantes", true, "")
        ),
        events = listOf(
            AttendanceEvent("05/11/2026 14:03", "Servidor A", "Anotação", "Cobrado o retorno por telefone."),
            AttendanceEvent("27/10/2026 09:15", "Servidor B", "Entrada", "Atendimento recebido.")
        ),
        attachments = listOf(
            FileItem("attachment", 5, 0, "Pedido de apoio.pdf", 182_000, "Cópia do pedido"),
            FileItem("attachment", 6, 0, "Lista de participantes.xlsx", 24_300, "")
        )
    )

    val fichaSimples = AttendanceDetail(
        id = 4, reference = "ORG-MEM-2026/00077", subject = "Memorando sobre a escala de trabalho",
        statusLabel = "A distribuir", open = true, dueLabel = "", late = false, dueToday = false,
        fields = listOf(AttendanceField("Situação", "A distribuir"), AttendanceField("Prioridade", "Normal"), AttendanceField("Entrada", "09/11/2026")),
        flags = emptyList(), reviewFlags = emptyList(), documentId = 0, documentNumber = "", documentTitle = "",
        tasks = emptyList(), events = emptyList(), attachments = emptyList()
    )

    val result = SearchResult(
        documents = SearchGroup(
            listOf(
                SearchDoc(101, "ORG-OFI-2026/00431", "Solicitação de apoio logístico para evento", ""),
                SearchDoc(104, "", "Recibo de entrega de equipamentos de apoio", "Manual"),
                SearchDoc(102, "", "Convite: reunião do Comitê Gestor", "E-mail")
            ),
            more = 5
        ),
        tasks = SearchGroup(
            listOf(
                Samples.tasks[0], Samples.tasks[2],
                Samples.tasks[7]
            ),
            more = 0
        ),
        attendances = SearchGroup(attendances.take(2), more = 1)
    )

    val names = listOf("Servidor A", "Servidor B", "Sebastiana Lima", "Carlos Souza", "Servidor C")
}

/** Pictures of the Responsável suggestions, the atendimentos, the search, Ajustes and the first setup. */
class AtendimentosBuscaTest {
    @get:Rule val paparazzi = newPaparazzi()

    // ------------------------------------------------------------ Responsável

    private fun task(assignee: String): TaskDetail {
        val t = Samples.taskLinked
        return t.copy(form = t.form.copy(assignee = assignee))
    }

    private fun taskScreen(t: TaskDetail, me: String = "Servidor A"): @androidx.compose.runtime.Composable () -> Unit = {
        TaskScreen(
            t.id, TaskEditState(id = t.id, loading = false, task = t), Samples.actions, Samples.priorities,
            {}, {}, {}, {}, {}, {}, {}, MoreSamples.names, me
        )
    }

    @Test fun responsavelSugestoes() {
        paparazzi.tall(3700)
        paparazzi.shot("claro") { taskScreen(task("Se"), me = "Servidor A")() }
    }

    @Test fun responsavelSugestoesEscuro() {
        paparazzi.tall(3700)
        paparazzi.shot("escuro", dark = true) { taskScreen(task("Se"), me = "Servidor A")() }
    }

    @Test fun responsavelVazioComEu() {
        paparazzi.tall(3700)
        paparazzi.shot("vazio_com_eu") { taskScreen(task(""), me = "Servidor A")() }
    }

    @Test fun novaTarefaComNome() = paparazzi.shot("claro") {
        NewTaskScreen(
            "", Samples.actions.filter { it.key != "triar" }, {}, { _, _, _, _ -> }, withoutDocument = true,
            assignees = MoreSamples.names, myName = "Servidor A"
        )
    }

    // ------------------------------------------------------------ atendimentos

    private fun listShell(name: String, state: AttendancesState, query: String = "", dark: Boolean = false) {
        paparazzi.tall(2500)
        paparazzi.shotInShell(name, 1, dark, shell = Samples.shell.copy(attendances = true)) {
            Column(Modifier.fillMaxSize()) {
                SegmentedTabs(listOf("Tarefas", "Atendimentos"), 1, {})
                Box(Modifier.weight(1f).fillMaxWidth()) { AttendancesScreen(state, query, {}, {}) }
            }
        }
    }

    @Test fun atendimentosLista() = listShell("claro", MoreSamples.attendancesState)

    @Test fun atendimentosListaEscuro() = listShell("escuro", MoreSamples.attendancesState, dark = true)

    @Test fun atendimentosSemResultado() = listShell("sem_resultado", MoreSamples.attendancesState, query = "zzz")

    @Test fun atendimentosVazio() = listShell("vazio", AttendancesState(loading = false))

    @Test fun atendimentosFiltrados() = listShell("filtrado", MoreSamples.attendancesState, query = "servidor a")

    @Test fun tarefasComSegmento() {
        paparazzi.tall(2500)
        paparazzi.shotInShell("segmento_tarefas", 1) {
            Column(Modifier.fillMaxSize()) {
                SegmentedTabs(listOf("Tarefas", "Atendimentos"), 0, {})
                Box(Modifier.weight(1f).fillMaxWidth()) { TasksScreen(TasksState(false, null, Samples.tasks), {}, {}) }
            }
        }
    }

    @Test fun atendimentoFicha() {
        paparazzi.tall(4200)
        paparazzi.shot("claro") { AttendanceScreen(1, AttendanceViewState(1, false, null, MoreSamples.ficha), {}, {}, {}, {}) }
    }

    @Test fun atendimentoFichaEscuro() {
        paparazzi.tall(4200)
        paparazzi.shot("escuro", dark = true) { AttendanceScreen(1, AttendanceViewState(1, false, null, MoreSamples.ficha), {}, {}, {}, {}) }
    }

    @Test fun atendimentoFichaSimples() = paparazzi.shot("simples") {
        AttendanceScreen(4, AttendanceViewState(4, false, null, MoreSamples.fichaSimples), {}, {}, {}, {})
    }

    @Test fun atendimentoFichaErro() = paparazzi.shot("erro") {
        AttendanceScreen(
            9, AttendanceViewState(9, false, "Atendimento não encontrado. Ele pode ter sido excluído em outro aparelho."),
            {}, {}, {}, {}
        )
    }

    @Test fun hojeComAtendimentosToqueaveis() {
        paparazzi.tall(2600)
        paparazzi.shotInShell("hoje_atendimentos", 0, shell = Samples.shell.copy(badge = 0)) {
            TodayScreen(TodayState(loading = false, attendances = Samples.attendances), {}, {}, {})
        }
    }

    // ------------------------------------------------------------ busca

    private fun search(name: String, state: SearchState, dark: Boolean = false) {
        paparazzi.tall(3000)
        paparazzi.shot(name, dark) { SearchScreen(state, {}, {}, {}, {}, {}) }
    }

    @Test fun buscaComTresGrupos() = search("tres_grupos", SearchState(query = "apoio", result = MoreSamples.result))

    @Test fun buscaComTresGruposEscuro() = search("tres_grupos_escuro", SearchState(query = "apoio", result = MoreSamples.result), dark = true)

    @Test fun buscaSemResultado() = search("vazio", SearchState(query = "zzzz", result = SearchResult(SearchGroup.none(), SearchGroup.none(), SearchGroup.none())))

    @Test fun buscaInicial() = search("inicial", SearchState())

    @Test fun buscaBuscando() = search("buscando", SearchState(query = "apo", loading = true))

    // ------------------------------------------------------------ ajustes e primeira configuração

    private val named = Samples.settings.copy(personName = "Servidor A", noticeOn = true, noticeTime = "08:00")

    private fun settings(name: String, state: SettingsState, dark: Boolean = false) {
        paparazzi.tall(3900)
        paparazzi.shot(name, dark) { SettingsScreen(state, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) }
    }

    @Test fun ajustesComNomeEAviso() = settings("claro", named)

    @Test fun ajustesComNomeEAvisoEscuro() = settings("escuro", named, dark = true)

    @Test fun ajustesAvisoDesligado() = settings("aviso_desligado", Samples.settings.copy(personName = "", noticeOn = false))

    @Test fun ajustesAvisoSemPermissao() = settings("aviso_sem_permissao", Samples.settings.copy(personName = "Servidor A", noticeOn = false, noticeBlocked = true))

    @Test fun setupNome() = paparazzi.shot("nome") {
        SetupScreen(SetupState(step = 2, fileName = "ponte-sync.ponte", done = true, personName = "Servidor A", status = "Sincronizado: 42 itens novos e 0 atualizados recebidos • gravado no Drive"), {}, {}, {}, {}, {}, {}, {}, {})
    }
}
