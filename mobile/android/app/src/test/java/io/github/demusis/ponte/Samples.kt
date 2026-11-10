package io.github.demusis.ponte

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import app.cash.paparazzi.DeviceConfig
import app.cash.paparazzi.Paparazzi

// Sample data and helpers for the pictures of the screens (Paparazzi renders them on the
// computer, with no phone). Run: gradle recordPaparazziDebug; the PNGs land in
// app/src/test/snapshots/images.

internal fun newPaparazzi() = Paparazzi(
    deviceConfig = DeviceConfig.PIXEL_5,
    theme = "android:Theme.Material.Light.NoActionBar"
)

/** A taller phone, to see a long screen whole. */
internal fun Paparazzi.tall(heightPx: Int = 4400) {
    unsafeUpdateConfig(deviceConfig = DeviceConfig.PIXEL_5.copy(screenHeight = heightPx))
}

internal fun Paparazzi.shot(name: String, dark: Boolean = false, content: @Composable () -> Unit) {
    snapshot(name) { PonteTheme(dark = dark) { content() } }
}

/** One of the three areas inside its frame, as on the phone. */
internal fun Paparazzi.shotInShell(
    name: String, tab: Int, dark: Boolean = false, shell: ShellState = Samples.shell, content: @Composable () -> Unit
) {
    shot(name, dark) { MainShell(shell.copy(tab = tab), {}, {}, {}, {}) { content() } }
}

internal object Samples {
    val shell = ShellState(tab = 0, badge = 3, syncing = false, statusLine = "Sincronizado · 14:32", statusKind = StatusKind.OK)

    private fun task(
        id: Long, title: String, action: String, number: String, due: String = "", chip: String = "",
        done: Boolean = false, overdue: Boolean = false, today: Boolean = false
    ) = TaskItem(id, 100 + id, title, action, number, due, chip, done, overdue, today)

    val tasks = listOf(
        task(1, "Responder ofício sobre o pessoal do Projeto Alfa", "Responder", "OF. 0412/2026", "2026-11-05", "há 4 dias", overdue = true),
        task(2, "Enviar relatório mensal de atividades", "Encaminhar", "MEM. 118/2026", "2026-11-07", "há 2 dias", overdue = true),
        task(3, "Confirmar presença na reunião do Comitê Gestor", "Responder", "Convite reunião Comitê Gestor", "2026-11-09", "hoje", today = true),
        task(4, "Conferir planilha de veículos", "Analisar", "PROC. 2026/5512", "2026-11-12", "12/11"),
        task(5, "Preparar minuta de resposta ao Setor Jurídico", "Elaborar minuta", "OF. 0398/2026", "2026-11-20", "20/11"),
        task(6, "Arquivar comprovante de entrega", "Arquivar", "Recibo de entrega", ""),
        task(7, "Ligar para o setor de protocolo", "", "", ""),
        task(8, "Assinar termo de recebimento", "Assinar", "TERMO 22/2026", "2026-11-03", "03/11", done = true),
        task(9, "Encaminhar escala de trabalho", "Encaminhar", "MEM. 101/2026", "", "", done = true)
    )

    private fun urgent(t: TaskItem, key: String, label: String) = t.copy(priority = key, priorityLabel = label)

    /** A task without document: "Sem documento" instead of the number. */
    private fun loose(id: Long, title: String, action: String, due: String, chip: String, overdue: Boolean = false, today: Boolean = false) =
        TaskItem(id, 0, title, action, "", due, chip, false, overdue, today)

    val todayLate = listOf(
        urgent(tasks[0], "urgente", "Urgente"),
        tasks[1],
        loose(10, "Ligar para o setor de protocolo", "Acompanhar", "2026-11-09", "ontem", overdue = true)
    )
    val todayDue = listOf(urgent(tasks[2], "alta", "Alta"))
    val todayUpcoming = listOf(
        tasks[3],
        loose(11, "Reservar a sala para a reunião", "Examinar", "2026-11-13", "13/11"),
        urgent(tasks[4], "baixa", "Baixa")
    )
    val attendances = listOf(
        AttendanceItem(1, "ORG-OFI-2026/00431", "Pedido de apoio logístico para evento", "Em análise", "2026-11-06", "há 4 dias", true),
        AttendanceItem(2, "ORG-PRO-2026/00212", "Consulta sobre o quadro de pessoal", "Aguardando resposta", "2026-11-10", "hoje", false)
    )
    val today = TodayState(
        loading = false, late = todayLate, today = todayDue, upcoming = todayUpcoming, attendances = attendances
    )

    private fun detailOf(
        id: Long, documentId: Long, number: String, title: String, form: TaskForm, done: Boolean = false,
        checklist: List<ChecklistItem> = emptyList()
    ) = TaskDetail(id, documentId, number, title, form, done, 3, checklist)

    val taskForm = TaskForm(
        "Responder ofício sobre o pessoal do Projeto Alfa", "responder", "alta", "2026-11-12", "Servidor A",
        "Pedir os valores de diárias à coordenação antes de responder."
    )

    val taskLinked = detailOf(
        1, 101, "ORG-OFI-2026/00431", "Solicitação de informações sobre o pessoal do Projeto Alfa", taskForm,
        checklist = listOf(
            ChecklistItem(true, "Conferir o quadro de pessoal"),
            ChecklistItem(false, "Reunir os valores de diárias"),
            ChecklistItem(false, "Enviar a minuta à chefia")
        )
    )

    val taskLoose = detailOf(
        10, 0, "", "", TaskForm("Ligar para o setor de protocolo", "acompanhar", "normal", "2026-11-09", "", "")
    )

    val priorities = listOf(Choice("baixa", "Baixa"), Choice("normal", "Normal"), Choice("alta", "Alta"), Choice("urgente", "Urgente"))

    val docs = listOf(
        Doc(
            101, "OF. 0412/2026", "Solicitação de informações sobre o pessoal do Projeto Alfa", "Aguardando dados de outro setor.", null,
            type = "Ofício", date = "03/11/2026", origin = "Diretoria-Geral", openTasks = 1,
            taskDueLabel = "05/11/2026", taskOverdue = true, attachments = 2
        ),
        Doc(
            102, "", "Convite: reunião do Comitê Gestor", "", null,
            type = "E-mail recebido", date = "08/11/2026", email = true, emailFrom = "Comitê Gestor <comite@exemplo.org>", openTasks = 1,
            taskDueLabel = "09/11/2026", taskDueToday = true, attachments = 1
        ),
        Doc(
            103, "MEM. 118/2026", "Relatório mensal de atividades — outubro", "", null,
            type = "Memorando", date = "30/10/2026", origin = "Coordenação de Planejamento", openTasks = 2,
            taskDueLabel = "07/11/2026", taskOverdue = true
        ),
        Doc(
            104, "", "Recibo de entrega de equipamentos", "Foto do recibo assinado.", null,
            type = "Papel", date = "06/11/2026", origin = "Almoxarifado central", manual = true, attachments = 1
        ),
        Doc(
            105, "PROC. 2026/5512", "Aquisição de veículos — planilha de distribuição", "", null,
            type = "Processo", date = "28/10/2026", origin = "Superintendência de Logística", openTasks = 1, taskDueLabel = "12/11/2026"
        ),
        Doc(
            106, "DESP. 77/2026", "Despacho de encaminhamento", "", "nota ilegível neste aparelho",
            type = "Despacho", date = "21/10/2026", origin = "Gabinete"
        )
    )

    val detail = DocDetail(
        id = 101,
        number = "OF. 0412/2026",
        title = "Solicitação de informações sobre o pessoal do Projeto Alfa",
        type = "Ofício",
        date = "03/11/2026",
        origin = "Diretoria-Geral",
        note = "Aguardando dados de outro setor. Cobrar a chefia na segunda-feira.",
        analysis = Analysis(
            summary = "A Diretoria-Geral solicita, em até cinco dias úteis, o número de servidores alocados no Projeto Alfa " +
                "entre agosto e outubro, com a distribuição por unidade e os custos de diárias.",
            priority = "alta",
            reason = "Prazo curto e pedido vindo da Diretoria-Geral.",
            deadline = "5 dias úteis a contar do recebimento",
            suggested = listOf(
                Suggestion("Responder", "O ofício pede informações com prazo."),
                Suggestion("Encaminhar", "Os dados estão com outro setor.")
            )
        ),
        tasks = listOf(tasks[0], tasks[7].copy(documentId = 101)),
        files = listOf(
            FileItem("pdf", 1, 1, "Ofício 0412-2026.pdf", 248_000, ""),
            FileItem("attachment", 2, 0, "Anexo I - Quadro de pessoal.xlsx", 31_500, "Planilha com o quadro por unidade"),
            FileItem("attachment", 3, 0, "Anexo II - Portaria.pdf", 1_420_000, "")
        ),
        text = "ÓRGÃO DE EXEMPLO\nDIRETORIA-GERAL\n\nOfício nº 0412/2026\n\nSenhor Coordenador,\n\n" +
            "Cumprimentando-o cordialmente, solicito a Vossa Senhoria que informe, no prazo de 5 (cinco) dias úteis, o quantitativo " +
            "de servidores alocados no Projeto Alfa no período de agosto a outubro do corrente ano, discriminando " +
            "a distribuição por unidade e os valores despendidos com diárias.\n\nAs informações subsidiarão o relatório a ser " +
            "apresentado ao Conselho Gestor.\n\nAtenciosamente,\n\nDiretor de Área"
    )

    val actions = listOf(
        Choice("triar", "Triar"), Choice("responder", "Responder"), Choice("encaminhar", "Encaminhar"),
        Choice("acompanhar", "Acompanhar"), Choice("examinar", "Examinar")
    )

    val kinds = listOf(Choice("papel", "Papel"), Choice("telefonema", "Telefonema"), Choice("reuniao", "Reunião"), Choice("outro", "Outro"))

    /** A stand-in for the picture taken: a sheet of paper on a table. */
    fun photo(): ImageBitmap {
        val b = Bitmap.createBitmap(600, 800, Bitmap.Config.ARGB_8888)
        val c = Canvas(b)
        val p = Paint(Paint.ANTI_ALIAS_FLAG)
        p.color = 0xFF6B5B4B.toInt()
        c.drawRect(0f, 0f, 600f, 800f, p)
        p.color = 0xFFF7F4EC.toInt()
        c.drawRect(70f, 60f, 530f, 740f, p)
        p.color = 0xFF8A8F96.toInt()
        var y = 130f
        while (y < 680f) {
            c.drawRect(110f, y, if (y % 90f < 30f) 380f else 490f, y + 10f, p)
            y += 30f
        }
        return b.asImageBitmap()
    }

    val accounts = listOf(
        MailAccount("a1", "Institucional", "imap.gmail.com", 993, "servidor@exemplo.org", true, true, "INBOX", "todas", "2026-11-09T12:00:00Z"),
        MailAccount("a2", "", "imap.gmail.com", 993, "conta@exemplo.org", false, false, "INBOX", "estrela", "2026-10-02T08:00:00Z")
    )

    val folders = listOf(MailFolder("INBOX", "Caixa de entrada"), MailFolder("[Gmail]/Com estrela", "Com estrela"), MailFolder("Oficios", "Ofícios"))

    val messages = listOf(
        MailMessage(901, "09/11/2026 10:42", "Comitê Gestor <comite@exemplo.org>", "Convite: reunião do Comitê Gestor", true, false),
        MailMessage(900, "09/11/2026 08:15", "Protocolo Geral <protocolo@exemplo.org>", "Ofício 0431/2026 - Solicitação de apoio logístico para evento", false, false),
        MailMessage(899, "08/11/2026 17:03", "Remetente de exemplo <remetente@exemplo.org>", "RES: Quadro de pessoal do Projeto Alfa", false, true),
        MailMessage(898, "08/11/2026 11:20", "Setor Jurídico <juridico@exemplo.org>", "", false, false),
        MailMessage(897, "07/11/2026 16:48", "Superintendência de Logística <logistica@exemplo.org>", "Planilha de distribuição de veículos (versão final)", true, false, error = "anexo acima do limite"),
        MailMessage(896, "07/11/2026 09:02", "Boletim Diário <boletim@exemplo.org>", "Boletim diário de 06/11", false, false)
    )

    val mail = MailState(
        loading = false, accounts = accounts, account = accounts[0], folders = folders, folder = "INBOX",
        filter = "todas", messages = messages, listed = true, checked = setOf(901L, 900L)
    )

    val settings = SettingsState(
        fileName = "ponte-sync.ponte", fileSize = 48_300_000, passphrase = "", canRemember = true, remembered = true,
        mobileData = false, accounts = accounts, keyBroken = false, pending = false, version = "0.3.0",
        lastStatus = "Sincronizado: 2 itens novos e 0 atualizados recebidos • 1 alteração deste aparelho enviada • gravado no Drive", lastStatusError = false
    )
}
