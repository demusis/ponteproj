package io.github.demusis.ponte

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.ui.Modifier
import app.cash.paparazzi.DeviceConfig
import org.junit.Rule
import org.junit.Test

/** Sample data of the Recursos screens (all generic). The clock of the pictures is [NOW]. */
internal object ResourceSamples {
    /** Tuesday 10/11/2026, 10:00. */
    const val NOW = "2026-11-10T10:00"

    private fun resource(
        uid: String, name: String, quantity: Int = 1, location: String = "",
        identification: String = "", notes: String = "", active: Boolean = true
    ) = ResourceItem(
        uid, name, quantity, location, identification, notes,
        if (active) "ativo" else "fora_de_uso", if (active) "Ativo" else "Fora de uso", active
    )

    /** What the core says of each resource at [NOW]: taken, free text, the uids of today's and next uses. */
    private class Facts(val taken: Int, val today: List<String>, val next: List<String>)

    private val facts = mapOf(
        "r1" to Facts(0, listOf("u3"), listOf("u3", "u5", "u7")),
        "r2" to Facts(0, emptyList(), listOf("u4")),
        "r3" to Facts(1, listOf("u1"), listOf("u1")),
        "r4" to Facts(1, listOf("u2"), listOf("u2")),
        "r5" to Facts(0, emptyList(), listOf("u6")),
        "r6" to Facts(0, emptyList(), emptyList()),
        "r7" to Facts(0, emptyList(), emptyList())
    )

    private fun withFacts(r: ResourceItem): ResourceItem {
        val f = facts.getValue(r.uid)
        val free = (r.quantity - f.taken).coerceAtLeast(0)
        return r.copy(
            inUseNow = f.taken, freeNow = free, usageLine = "${f.taken} de ${r.quantity} em uso agora",
            freeText = if (free == 1) "1 livre" else "$free livres", todayUids = f.today, nextUids = f.next
        )
    }

    // By name, as the core lists them.
    val resources = listOf(
        resource("r2", "Auditório", 1, "Térreo, bloco B"),
        resource("r5", "Cadeiras empilháveis", 40, "Depósito"),
        resource("r7", "Câmera antiga", 1, "Almoxarifado", "Patrimônio 000981", active = false),
        resource("r6", "Manual de redação oficial", 3, "Biblioteca", "ISBN 000-00-0000-000-0"),
        resource("r4", "Notebook de apresentações", 2, "Almoxarifado", "Patrimônio 004520"),
        resource("r3", "Projetor", 1, "Almoxarifado", "Patrimônio 004512", "Cabo HDMI na caixa."),
        resource(
            "r1", "Sala de reuniões do 2º andar", 1, "2º andar, bloco A", "",
            "A chave fica na portaria."
        )
    ).map { withFacts(it) }

    /** The period as the core words it (same day: "10/11 14:00–16:00"; several days: "11/11 08:00 – 12/11 18:00"). */
    private fun periodOf(start: String, end: String): String {
        fun day(m: String) = "${m.substring(8, 10)}/${m.substring(5, 7)}"
        fun clock(m: String) = m.substring(11, 16)
        return if (start.take(10) == end.take(10)) "${day(start)} ${clock(start)}–${clock(end)}"
        else "${day(start)} ${clock(start)} – ${day(end)} ${clock(end)}"
    }

    private val labels = mapOf("reservado" to "Reservado", "em_uso" to "Em uso", "devolvido" to "Devolvido", "cancelado" to "Cancelado")

    /** A use of [resource] (the room by default); `overdue` as the core would say it at [NOW]. */
    fun use(
        uid: String, resource: ResourceItem = resources.first { it.uid == "r1" }, who: String = "Servidor A",
        sector: String = "", purpose: String = "", status: String = "reservado",
        start: String = "2026-11-10T09:00", end: String = "2026-11-10T10:00", quantity: Int = 1,
        returnedAt: String = "", notes: String = ""
    ) = ResourceUseItem(
        uid = uid, resourceUid = resource.uid, resourceName = resource.name,
        quantity = quantity, who = who, sector = sector, purpose = purpose, start = start, end = end,
        returnedAt = returnedAt, status = status, statusLabel = labels.getValue(status), notes = notes,
        overdue = status == "em_uso" && end < NOW, period = periodOf(start, end)
    )

    private fun r(uid: String) = resources.first { it.uid == uid }

    val uses = listOf(
        use(
            "u1", r("r3"), "Servidor A", "Setor Administrativo", "Apresentação no auditório", "em_uso",
            "2026-11-09T09:00", "2026-11-09T17:00"
        ),
        use("u2", r("r4"), "Servidor C", "Setor Jurídico", "Audiência externa", "em_uso", "2026-11-10T08:00", "2026-11-10T18:00"),
        use(
            "u3", r("r1"), "Servidor B", "Setor Jurídico", "Reunião de equipe", "reservado", "2026-11-10T14:00", "2026-11-10T16:00",
            notes = "Pedir café para dez pessoas."
        ).copy(
            documentId = 101, documentNumber = "ORG-OFI-2026/00431", documentTitle = "Solicitação de apoio logístico",
            taskId = 5, taskTitle = "Devolver as chaves da sala"
        ),
        use("u4", r("r2"), "Servidor A", "Setor Jurídico", "Palestra de integração", "reservado", "2026-11-12T09:00", "2026-11-12T12:00"),
        use("u5", r("r1"), "Servidor C", "Setor Administrativo", "Oficina de planejamento", "reservado", "2026-11-13T09:00", "2026-11-13T11:00"),
        use(
            "u6", r("r5"), "Servidor B", "Setor Administrativo", "Evento de fim de ano", "reservado", "2026-11-11T08:00", "2026-11-12T18:00", 30
        ),
        use("u7", r("r1"), "Servidor A", "Setor Jurídico", "Reunião de balanço", "reservado", "2026-12-15T14:00", "2026-12-15T16:00"),
        use(
            "u8", r("r3"), "Servidor B", "Setor Jurídico", "Reunião externa", "devolvido", "2026-11-05T09:00", "2026-11-05T17:00",
            returnedAt = "2026-11-05T16:40"
        ),
        use("u9", r("r2"), "Servidor C", "", "", "cancelado", "2026-11-08T09:00", "2026-11-08T11:00")
    )

    private fun group(key: String, title: String, vararg uids: String) =
        ResourceGroup(key, title, key == "atrasados", uids.toList())

    /** The core's groups at [NOW] for no search. */
    val state = ResourcesState(
        loading = false, uses = uses, resources = resources,
        groups = listOf(
            group("atrasados", "Atrasados", "u1"), group("em_uso", "Em uso agora", "u2"), group("hoje", "Hoje", "u3"),
            group("proximos", "Próximos dias", "u6", "u4", "u5"), group("adiante", "Mais adiante", "u7"),
            group("encerrados", "Mais antigos e encerrados", "u9", "u8")
        ),
        rows = listOf("r2", "r5", "r6", "r4", "r3", "r1", "r7")
    )

    /** What the core answers for a search that finds nothing, and for "auditorio". */
    val stateNothing = state.copy(groups = emptyList(), rows = emptyList())
    val stateAuditorium = state.copy(
        groups = listOf(group("proximos", "Próximos dias", "u4"), group("encerrados", "Mais antigos e encerrados", "u9")),
        rows = listOf("r2")
    )
}

/** Pictures of the Recursos area: the list (uses and register), the two details, Hoje, and the bar of three names. */
class RecursosTelasTest {
    @get:Rule val paparazzi = newPaparazzi()


    private fun list(
        name: String, state: ResourcesState, view: Int = 0, query: String = "", dark: Boolean = false, height: Int = 3000
    ) {
        paparazzi.tall(height)
        paparazzi.shotInShell(name, 1, dark, shell = Samples.shell.copy(resources = true)) {
            ResourcesScreen(state, query, {}, view, {}, {}, {})
        }
    }

    @Test fun usosLista() = list("usos_claro", ResourceSamples.state)

    @Test fun usosListaEscuro() = list("usos_escuro", ResourceSamples.state, dark = true)

    @Test fun usosBuscaSemResultado() = list("usos_sem_resultado", ResourceSamples.stateNothing, query = "zzz", height = 1800)

    @Test fun usosBuscaComEncerrado() = list("usos_busca_auditorio", ResourceSamples.stateAuditorium, query = "auditorio", height = 2000)

    @Test fun usosVazio() = list("usos_vazio", ResourcesState(loading = false), height = 1800)

    @Test fun recursosLista() = list("recursos_claro", ResourceSamples.state, view = 1)

    @Test fun recursosListaEscuro() = list("recursos_escuro", ResourceSamples.state, view = 1, dark = true)

    @Test fun recursosVazio() = list("recursos_vazio", ResourcesState(loading = false), view = 1, height = 1800)

    private fun useDetail(name: String, uid: String, dark: Boolean = false, state: ResourcesState = ResourceSamples.state) {
        paparazzi.tall(2300)
        paparazzi.shot(name, dark) { ResourceUseScreen(uid, state, {}, {}, {}, {}, {}) }
    }

    @Test fun usoAtrasado() = useDetail("uso_atrasado", "u1")

    @Test fun usoReservadoComVinculos() = useDetail("uso_reservado", "u3")

    @Test fun usoReservadoComVinculosEscuro() = useDetail("uso_reservado_escuro", "u3", dark = true)

    @Test fun usoDevolvido() = useDetail("uso_devolvido", "u8")

    @Test fun usoQueNaoExisteMais() = useDetail("uso_inexistente", "nenhum")

    private fun resourceDetail(name: String, uid: String, dark: Boolean = false) {
        paparazzi.tall(2300)
        paparazzi.shot(name, dark) { ResourceScreen(uid, ResourceSamples.state, {}, {}) }
    }

    @Test fun recursoSala() = resourceDetail("recurso_sala", "r1")

    @Test fun recursoSalaEscuro() = resourceDetail("recurso_sala_escuro", "r1", dark = true)

    @Test fun recursoComVariasUnidades() = resourceDetail("recurso_notebooks", "r4")

    @Test fun recursoForaDeUso() = resourceDetail("recurso_fora_de_uso", "r7")

    private val busy = TodayState(
        loading = false, late = Samples.todayLate, today = Samples.todayDue, upcoming = emptyList(),
        attendances = Samples.attendances, reservationsToday = 2, overdueReturns = 1,
        summary = "3 tarefas atrasadas · 1 para hoje · 2 atendimentos com prazo vencido ou para hoje · 2 reservas de hoje · 1 devolução atrasada"
    )

    @Test fun hojeComRecursos() {
        paparazzi.tall(3300)
        paparazzi.shotInShell("hoje_claro", 0) { TodayScreen(busy, {}, {}) }
    }

    @Test fun hojeComRecursosEscuro() {
        paparazzi.tall(3300)
        paparazzi.shotInShell("hoje_escuro", 0, dark = true) { TodayScreen(busy, {}, {}) }
    }

    @Test fun hojeSoComRecursos() = paparazzi.shotInShell(
        "hoje_so_recursos", 0, shell = Samples.shell.copy(badge = 0)
    ) { TodayScreen(TodayState(loading = false, reservationsToday = 1, overdueReturns = 2, summary = "1 reserva de hoje · 2 devoluções atrasadas"), {}, {}) }

    // The three names of the segmented control on a narrow screen and with a large font.
    private fun small(fontScale: Float) = DeviceConfig.PIXEL_5.copy(
        screenWidth = 720, screenHeight = 1440, xdpi = 320, ydpi = 320,
        density = com.android.resources.Density.XHIGH, fontScale = fontScale
    )

    private fun narrow(name: String, scale: Float, dark: Boolean = false) {
        paparazzi.unsafeUpdateConfig(deviceConfig = small(scale))
        paparazzi.shotInShell(name, 1, dark, shell = Samples.shell.copy(resources = true)) {
            ResourcesScreen(ResourceSamples.state, "", {}, 0, {}, {}, {})
        }
    }

    @Test fun tresNomesEmTelaEstreita() = narrow("estreita", 1f)

    @Test fun tresNomesComLetraGrande() = narrow("letra_grande", 1.3f)

    @Test fun tresNomesComLetraMuitoGrandeEscuro() = narrow("letra_muito_grande_escuro", 1.6f, dark = true)
}
