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
        task(1, "Responder ofício sobre o efetivo da Operação Fronteira", "Responder", "OF. 0412/2026", "2026-11-05", "há 4 dias", overdue = true),
        task(2, "Enviar relatório mensal de ocorrências", "Encaminhar", "MEM. 118/2026", "2026-11-07", "há 2 dias", overdue = true),
        task(3, "Confirmar presença na reunião do GGI", "Responder", "Convite reunião GGI", "2026-11-09", "hoje", today = true),
        task(4, "Conferir planilha de viaturas", "Analisar", "PROC. 2026/5512", "2026-11-12", "12/11"),
        task(5, "Preparar minuta de resposta à Corregedoria", "Elaborar minuta", "OF. 0398/2026", "2026-11-20", "20/11"),
        task(6, "Arquivar comprovante de entrega", "Arquivar", "Recibo de entrega", ""),
        task(7, "Ligar para o setor de protocolo", "", "", ""),
        task(8, "Assinar termo de recebimento", "Assinar", "TERMO 22/2026", "2026-11-03", "03/11", done = true),
        task(9, "Encaminhar escala de serviço", "Encaminhar", "MEM. 101/2026", "", "", done = true)
    )

    val docs = listOf(
        Doc(
            101, "OF. 0412/2026", "Solicitação de informações sobre o efetivo da Operação Fronteira", "Aguardando dados do 2º BPM.", null,
            type = "Ofício", date = "03/11/2026", origin = "Secretaria de Estado de Segurança Pública", openTasks = 1,
            taskDueLabel = "05/11/2026", taskOverdue = true, attachments = 2
        ),
        Doc(
            102, "", "Convite: reunião do Gabinete de Gestão Integrada", "", null,
            type = "E-mail recebido", date = "08/11/2026", email = true, emailFrom = "Gabinete GGI <ggi@sesp.mt.gov.br>", openTasks = 1,
            taskDueLabel = "09/11/2026", taskDueToday = true, attachments = 1
        ),
        Doc(
            103, "MEM. 118/2026", "Relatório mensal de ocorrências — outubro", "", null,
            type = "Memorando", date = "30/10/2026", origin = "Coordenadoria de Estatística", openTasks = 2,
            taskDueLabel = "07/11/2026", taskOverdue = true
        ),
        Doc(
            104, "", "Recibo de entrega de equipamentos", "Foto do recibo assinado.", null,
            type = "Papel", date = "06/11/2026", origin = "Almoxarifado central", manual = true, attachments = 1
        ),
        Doc(
            105, "PROC. 2026/5512", "Aquisição de viaturas — planilha de distribuição", "", null,
            type = "Processo", date = "28/10/2026", origin = "Superintendência de Logística", openTasks = 1, taskDueLabel = "12/11/2026"
        ),
        Doc(
            106, "DESP. 77/2026", "Despacho de encaminhamento", "", "nota ilegível neste aparelho",
            type = "Despacho", date = "21/10/2026", origin = "Gabinete do Secretário Adjunto"
        )
    )

    val detail = DocDetail(
        id = 101,
        number = "OF. 0412/2026",
        title = "Solicitação de informações sobre o efetivo da Operação Fronteira",
        type = "Ofício",
        date = "03/11/2026",
        origin = "Secretaria de Estado de Segurança Pública",
        note = "Aguardando dados do 2º BPM. Cobrar o comandante na segunda-feira.",
        analysis = Analysis(
            summary = "A Secretaria solicita, em até cinco dias úteis, o quantitativo de policiais empregados na Operação Fronteira " +
                "entre agosto e outubro, com a distribuição por unidade e os custos de diárias.",
            priority = "alta",
            reason = "Prazo curto e pedido vindo do gabinete do Secretário.",
            deadline = "5 dias úteis a contar do recebimento",
            suggested = listOf(
                Suggestion("Responder", "O ofício pede informações com prazo."),
                Suggestion("Encaminhar", "Os dados estão com o 2º BPM.")
            )
        ),
        tasks = listOf(tasks[0], tasks[7].copy(documentId = 101)),
        files = listOf(
            FileItem("pdf", 1, 1, "Ofício 0412-2026.pdf", 248_000, ""),
            FileItem("attachment", 2, 0, "Anexo I - Quadro de efetivo.xlsx", 31_500, "Planilha com o quadro por unidade"),
            FileItem("attachment", 3, 0, "Anexo II - Portaria.pdf", 1_420_000, "")
        ),
        text = "GOVERNO DO ESTADO\nSECRETARIA DE ESTADO DE SEGURANÇA PÚBLICA\n\nOfício nº 0412/2026\n\nSenhor Comandante,\n\n" +
            "Cumprimentando-o cordialmente, solicito a Vossa Senhoria que informe, no prazo de 5 (cinco) dias úteis, o quantitativo " +
            "de policiais militares empregados na Operação Fronteira no período de agosto a outubro do corrente ano, discriminando " +
            "a distribuição por unidade e os valores despendidos com diárias.\n\nAs informações subsidiarão o relatório a ser " +
            "apresentado ao Conselho Estadual de Segurança Pública.\n\nAtenciosamente,\n\nSecretário Adjunto de Integração Operacional"
    )

    val actions = listOf(
        Choice("responder", "Responder"), Choice("encaminhar", "Encaminhar"), Choice("analisar", "Analisar"),
        Choice("arquivar", "Arquivar"), Choice("outra", "Outra")
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
        MailAccount("a1", "Institucional", "imap.gmail.com", 993, "carlos.souza@sesp.mt.gov.br", true, true, "INBOX", "todas", "2026-11-09T12:00:00Z"),
        MailAccount("a2", "", "imap.gmail.com", 993, "carlos.pessoal@gmail.com", false, false, "INBOX", "estrela", "2026-10-02T08:00:00Z")
    )

    val folders = listOf(MailFolder("INBOX", "Caixa de entrada"), MailFolder("[Gmail]/Com estrela", "Com estrela"), MailFolder("Oficios", "Ofícios"))

    val messages = listOf(
        MailMessage(901, "09/11/2026 10:42", "Gabinete GGI <ggi@sesp.mt.gov.br>", "Convite: reunião do Gabinete de Gestão Integrada", true, false),
        MailMessage(900, "09/11/2026 08:15", "Protocolo Geral <protocolo@sesp.mt.gov.br>", "Ofício 0431/2026 - Solicitação de apoio logístico para evento", false, false),
        MailMessage(899, "08/11/2026 17:03", "Maria Albuquerque <maria.albuquerque@pm.mt.gov.br>", "RES: Quadro de efetivo da Operação Fronteira", false, true),
        MailMessage(898, "08/11/2026 11:20", "Corregedoria <corregedoria@sesp.mt.gov.br>", "", false, false),
        MailMessage(897, "07/11/2026 16:48", "Superintendência de Logística <logistica@sesp.mt.gov.br>", "Planilha de distribuição de viaturas (versão final)", true, false, error = "anexo acima do limite"),
        MailMessage(896, "07/11/2026 09:02", "Boletim Diário <boletim@sesp.mt.gov.br>", "Boletim de ocorrências de 06/11", false, false)
    )

    val mail = MailState(
        loading = false, accounts = accounts, account = accounts[0], folders = folders, folder = "INBOX",
        filter = "todas", messages = messages, listed = true, checked = setOf(901L, 900L)
    )

    val settings = SettingsState(
        fileName = "ponte-sync.ponte", fileSize = 48_300_000, passphrase = "", canRemember = true, remembered = true,
        mobileData = false, accounts = accounts, keyBroken = false, pending = false, version = "0.3.0",
        lastStatus = "Sincronizado: 2 documentos recebidos, 1 tarefa enviada (4,2 s, 48300000 bytes)", lastStatusError = false
    )
}
