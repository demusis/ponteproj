package io.github.demusis.ponte

import androidx.compose.ui.graphics.ImageBitmap
import org.json.JSONObject
import java.time.LocalDate
import java.time.temporal.ChronoUnit

// What the screens show. Plain values, built from the core's JSON by the controller, so every
// screen is a function of its state and can be rendered on the computer with sample data.

/** A task, in the Tarefas list or on a document's ficha. */
internal data class TaskItem(
    val id: Long,
    val documentId: Long,
    val title: String,
    /** "Responder", "Arquivar"… May be empty. */
    val actionLabel: String,
    /** Number (or short name) of the document it belongs to. May be empty. */
    val documentNumber: String,
    /** ISO date (AAAA-MM-DD) or empty. */
    val dueDate: String,
    /** Short text for the due chip: "há 2 dias", "hoje", "amanhã", "12/11". Empty without a due date. */
    val dueChip: String,
    val done: Boolean,
    val overdue: Boolean,
    val dueToday: Boolean,
    /** The core's key: "baixa", "normal", "alta" or "urgente". */
    val priority: String = "normal",
    /** "Baixa", "Alta", "Urgente"… */
    val priorityLabel: String = "",
    val notes: String = ""
) {
    /** The second line of a row: the action and the document ("Sem documento" for none). */
    val secondLine: String
        get() = listOf(actionLabel, if (documentId <= 0) "Sem documento" else documentNumber)
            .filter { it.isNotEmpty() }.joinToString(" · ")
}

/** Short, relative wording of a due date for the chip. */
internal fun dueChipText(iso: String, today: LocalDate = LocalDate.now()): String {
    val d = try { LocalDate.parse(iso) } catch (_: Throwable) { return iso }
    val days = ChronoUnit.DAYS.between(today, d)
    return when {
        days == 0L -> "hoje"
        days == 1L -> "amanhã"
        days == -1L -> "ontem"
        days < 0 -> "há ${-days} dias"
        d.year == today.year -> "%02d/%02d".format(d.dayOfMonth, d.monthValue)
        else -> "%02d/%02d/%d".format(d.dayOfMonth, d.monthValue, d.year)
    }
}

internal fun JSONObject.toTask(): TaskItem {
    val due = s("due_date")
    return TaskItem(
        id = optLong("id"),
        documentId = optLong("document_id"),
        title = s("title"),
        actionLabel = s("action_label"),
        documentNumber = s("document_number"),
        dueDate = due,
        dueChip = if (due.isEmpty()) "" else dueChipText(due),
        done = s("status") == "concluida",
        overdue = optBoolean("overdue"),
        dueToday = optBoolean("due_today"),
        priority = s("priority", "normal").ifEmpty { "normal" },
        priorityLabel = s("priority_label"),
        notes = s("notes")
    )
}

/** An atendimento with a deadline (late or due today), read-only on the phone. */
internal data class AttendanceItem(
    val id: Long,
    /** Reference, or the subject when there is none. */
    val reference: String,
    val subject: String,
    val statusLabel: String,
    val dueDate: String,
    val dueChip: String,
    val late: Boolean
)

internal fun JSONObject.toAttendance() = AttendanceItem(
    id = optLong("id"), reference = s("reference"), subject = s("subject"),
    statusLabel = s("status_label"), dueDate = s("due_date"),
    dueChip = s("due_date").let { if (it.isEmpty()) "" else dueChipText(it) }, late = optBoolean("late")
)

/** "1 tarefa atrasada · 2 para hoje", or "Nada vence hoje." when nothing is late or due today. */
internal fun todaySummary(late: Int, today: Int): String = when {
    late > 0 && today > 0 -> PhoneWords.plural(late, "tarefa atrasada", "tarefas atrasadas") + " · $today para hoje"
    late > 0 -> PhoneWords.plural(late, "tarefa atrasada", "tarefas atrasadas")
    today > 0 -> PhoneWords.plural(today, "tarefa para hoje", "tarefas para hoje")
    else -> "Nada vence hoje."
}

/** The checklist of a task (read-only here). */
internal data class ChecklistItem(val done: Boolean, val text: String)

/** The fields of a task the phone edits. */
internal data class TaskForm(
    val title: String,
    val action: String,
    val priority: String,
    /** ISO date or empty ("Sem prazo"). */
    val dueDate: String,
    val assignee: String,
    val notes: String
) {
    /** Whether the person changed something compared with [saved] (blanks around names do not count). */
    fun changedFrom(saved: TaskForm): Boolean = changes(saved).isNotEmpty()

    /** Field name (as the core's `saveTask` reads it) to new value, only for what differs. */
    fun changes(saved: TaskForm): Map<String, String> {
        val out = LinkedHashMap<String, String>()
        if (title.trim() != saved.title.trim()) out["title"] = title.trim()
        if (action != saved.action) out["action"] = action
        if (priority != saved.priority) out["priority"] = priority
        if (dueDate != saved.dueDate) out["due_date"] = dueDate
        if (assignee.trim() != saved.assignee.trim()) out["assignee"] = assignee.trim()
        if (notes != saved.notes) out["notes"] = notes
        return out
    }
}

/** Everything the task screen shows. [version] must come back with the save. */
internal data class TaskDetail(
    val id: Long,
    /** 0: a task without document. */
    val documentId: Long,
    val documentNumber: String,
    val documentTitle: String,
    val form: TaskForm,
    val done: Boolean,
    val version: Long,
    val checklist: List<ChecklistItem>
) {
    /** "ORG-… — assunto", or whichever of the two exists. */
    val documentLine: String
        get() = listOf(documentNumber, documentTitle).filter { it.isNotBlank() }.joinToString(" — ")

    /** As a row of a list, for the common actions (conclude, reopen). */
    fun asItem() = TaskItem(
        id = id, documentId = documentId, title = form.title, actionLabel = "", documentNumber = documentNumber,
        dueDate = form.dueDate, dueChip = "", done = done, overdue = false, dueToday = false
    )
}

internal fun JSONObject.toTaskDetail() = TaskDetail(
    id = optLong("id"), documentId = optLong("document_id"), documentNumber = s("document_number"),
    documentTitle = s("document_title"),
    form = TaskForm(
        title = s("title"), action = s("action"), priority = s("priority", "normal").ifEmpty { "normal" },
        dueDate = s("due_date"), assignee = s("assignee"), notes = s("notes")
    ),
    done = s("status") == "concluida", version = optLong("version"),
    checklist = optJSONArray("checklist").objs().map { ChecklistItem(it.optBoolean("done"), it.s("text")) }
)

/** Which documents the list shows. */
internal enum class DocFilter(val label: String) {
    ALL("Todos"), WITH_TASK("Com tarefa"), EMAILS("E-mails"), RECORDS("Registros")
}

/** A PDF of the document or an attachment. */
internal data class FileItem(
    /** "pdf" or "attachment" (what [openExported] takes). */
    val kind: String,
    val id: Long,
    val captureId: Long,
    val name: String,
    val size: Long,
    /** Only attachments have one; may be empty. */
    val description: String
)

internal data class Suggestion(val action: String, val reason: String)

/** The analysis made on the computer. Any field may be empty. */
internal data class Analysis(
    val summary: String,
    /** "alta", "média", "baixa"… as the core wrote it. */
    val priority: String,
    val reason: String,
    /** Deadline found in the text (free wording), or empty. */
    val deadline: String,
    val suggested: List<Suggestion>
)

/** Everything the ficha shows. */
internal data class DocDetail(
    val id: Long,
    val number: String,
    val title: String,
    /** "Ofício", "E-mail recebido", "Papel"… May be empty. */
    val type: String,
    /** dd/mm/aaaa or whatever the capture had. May be empty. */
    val date: String,
    /** Sender or origin, ready to show. May be empty. */
    val origin: String,
    val note: String,
    val analysis: Analysis?,
    val tasks: List<TaskItem>,
    val files: List<FileItem>,
    val text: String
)

internal fun JSONObject.toDocDetail(): DocDetail {
    val an = optJSONObject("analysis")
    val files = optJSONArray("files").objs().map {
        FileItem("pdf", it.optLong("id"), it.optLong("capture_id"), it.s("label", "PDF").ifEmpty { "PDF" }, it.optLong("size"), "")
    }
    val atts = optJSONArray("attachments").objs().map {
        FileItem("attachment", it.optLong("id"), 0L, it.s("name").ifEmpty { "Anexo" }, it.optLong("size"), it.s("description"))
    }
    val from = s("email_from")
    return DocDetail(
        id = optLong("id"),
        number = s("number"),
        title = s("title"),
        type = s("type"),
        date = formatDocDate(s("date")),
        origin = if (optBoolean("email") && from.isNotEmpty()) from else s("origin"),
        note = s("note"),
        analysis = an?.let { a ->
            Analysis(
                a.s("summary"), a.s("priority"), a.s("reason"), a.s("deadline"),
                a.optJSONArray("suggested").objs().map { Suggestion(it.s("action"), it.s("reason")) }
            )
        },
        tasks = optJSONArray("tasks").objs().map { it.toTask() },
        files = files + atts,
        text = s("text")
    )
}

/** AAAA-MM-DD → dd/mm/aaaa; anything else is kept. */
internal fun formatDocDate(value: String): String {
    val d = try { LocalDate.parse(value.trim()) } catch (_: Throwable) { return value.trim() }
    return "%02d/%02d/%d".format(d.dayOfMonth, d.monthValue, d.year)
}

/** A key and its label (task actions, kinds of record). */
internal data class Choice(val key: String, val label: String)

// ---------------------------------------------------------------- e-mail

internal data class MailAccount(
    val uid: String,
    val name: String,
    val host: String,
    val port: Int,
    val user: String,
    /** The app password is stored on this phone. */
    val hasPassword: Boolean,
    val remember: Boolean,
    /** Last folder listed, as the server names it. */
    val folder: String,
    /** Last filter: "todas", "estrela" or "nao_lidas". */
    val filter: String,
    /** RFC 3339 of the last listing, or empty. */
    val lastUsed: String
) {
    val label: String get() = name.ifEmpty { user }
}

internal fun JSONObject.toMailAccount() = MailAccount(
    uid = s("uid"), name = s("name"), host = s("host"), port = optInt("port", 993), user = s("user"),
    hasPassword = optBoolean("has_password"), remember = optBoolean("remember"),
    folder = s("folder", "INBOX").ifEmpty { "INBOX" }, filter = s("filter", "todas").ifEmpty { "todas" },
    lastUsed = s("last_used")
)

internal data class MailFolder(val raw: String, val name: String)

internal data class MailMessage(
    val uid: Long,
    /** As the server sent it, ready to show. */
    val date: String,
    val from: String,
    val subject: String,
    val flagged: Boolean,
    /** Already in the library ("Já no acervo"). */
    val imported: Boolean,
    /** Why the last import of this message failed, or null. */
    val error: String? = null
)

internal fun JSONObject.toMailMessage() = MailMessage(
    uid = optLong("uid"), date = s("date"), from = s("from"), subject = s("subject"),
    flagged = optBoolean("flagged"), imported = optBoolean("imported")
)

/** What the account form sends to be saved. */
internal data class AccountInput(
    val name: String,
    val host: String,
    val port: Int,
    val user: String,
    /** Empty keeps the stored one. */
    val password: String,
    val remember: Boolean
)

// ---------------------------------------------------------------- screen states

/** Color of the dot next to the one-line sync status. */
internal enum class StatusKind { OK, WAITING, ERROR, NEUTRAL }

/** The frame around the three areas. */
internal data class ShellState(
    /** 0 Hoje, 1 Tarefas, 2 Documentos, 3 E-mail. */
    val tab: Int = 0,
    /** Tasks late or due today (badge on Hoje); 0 hides it. */
    val badge: Int = 0,
    val syncing: Boolean = false,
    /** One line, already short ("Sincronizado · 14:32"). */
    val statusLine: String = "",
    val statusKind: StatusKind = StatusKind.NEUTRAL,
    /** False while the device key is broken. */
    val syncEnabled: Boolean = true
)

internal data class TasksState(
    val loading: Boolean = true,
    val error: String? = null,
    val tasks: List<TaskItem> = emptyList()
)

/** What the Hoje area lists, already ordered by the core. */
internal data class TodayState(
    val loading: Boolean = true,
    val error: String? = null,
    val late: List<TaskItem> = emptyList(),
    val today: List<TaskItem> = emptyList(),
    val upcoming: List<TaskItem> = emptyList(),
    val attendances: List<AttendanceItem> = emptyList()
) {
    val isEmpty: Boolean get() = late.isEmpty() && today.isEmpty() && upcoming.isEmpty() && attendances.isEmpty()
}

/** The task screen: the task as stored, and what is going on with it. */
internal data class TaskEditState(
    val loading: Boolean = true,
    /** Why the task could not be read. */
    val error: String? = null,
    val task: TaskDetail? = null,
    /** Saving, concluding or changing the document: buttons wait. */
    val busy: Boolean = false,
    /** Shown above the buttons (a failed save, a changed task). */
    val formError: String = ""
)

internal data class DocumentState(
    val loading: Boolean = true,
    val error: String? = null,
    val doc: DocDetail? = null
)

internal data class PhotoState(
    val kinds: List<Choice> = listOf(Choice("papel", "Papel")),
    val kind: String = "papel",
    val title: String = "",
    /** ISO date or empty ("sem data"). */
    val date: String = "",
    val origin: String = "",
    val text: String = "",
    /** The picture taken, small; null before the first one or while it loads. */
    val thumb: ImageBitmap? = null,
    val hasPhoto: Boolean = false,
    val saving: Boolean = false,
    /** "Preparando a foto…" while saving. */
    val progress: String = "",
    /** Shown in red above the footer. */
    val error: String = "",
    /** Under the title field ("Informe o título."). */
    val titleError: String = ""
)

internal data class MailState(
    val loading: Boolean = true,
    val accounts: List<MailAccount> = emptyList(),
    /** The account being browsed (null: none registered yet). */
    val account: MailAccount? = null,
    val folders: List<MailFolder> = emptyList(),
    /** Raw name of the chosen folder. */
    val folder: String = "INBOX",
    /** "todas", "estrela" or "nao_lidas". */
    val filter: String = "todas",
    val messages: List<MailMessage> = emptyList(),
    /** True after a listing came back (so an empty list means "no messages"). */
    val listed: Boolean = false,
    val checked: Set<Long> = emptySet(),
    /** A server conversation is going on. */
    val busy: Boolean = false,
    /** What it is doing, or the last result ("3 importadas"). */
    val info: String = "",
    val infoError: Boolean = false,
    /** The account has no stored password and none was typed in this session. */
    val needsPassword: Boolean = false
) {
    /** What the person reads for [folder]; the server is always asked by the raw name. */
    fun folderLabel(): String =
        folders.firstOrNull { it.raw == folder }?.name ?: if (folder.equals("INBOX", ignoreCase = true)) "Caixa de entrada" else folder
}

internal data class SettingsState(
    /** Name of the chosen Drive file, or empty when none. */
    val fileName: String = "",
    /** Its size in bytes, or -1. */
    val fileSize: Long = -1,
    /** The passphrase typed in this session (never saved). */
    val passphrase: String = "",
    /** The phone can keep the passphrase behind fingerprint/PIN. */
    val canRemember: Boolean = true,
    val remembered: Boolean = false,
    val mobileData: Boolean = false,
    val accounts: List<MailAccount> = emptyList(),
    val keyBroken: Boolean = false,
    /** Merged changes waiting to be written to the Drive. */
    val pending: Boolean = false,
    val version: String = "",
    /** Full text of the last sync status. */
    val lastStatus: String = "",
    val lastStatusError: Boolean = false,
    /** The phone has a secure screen lock, so the app lock can be used. */
    val lockAvailable: Boolean = true,
    /** The app lock is on (always false when [lockAvailable] is false). */
    val lockOn: Boolean = false,
    /** [LockDelay.key] of the chosen time. */
    val lockDelay: String = LockDelay.DEFAULT.key,
    /** Screenshots and the preview in the recent apps are blocked. */
    val secureScreen: Boolean = true
)

internal data class AccountFormState(
    /** null: new account. */
    val account: MailAccount? = null,
    val busy: Boolean = false,
    val error: String = ""
)

/** First setup, in three steps. */
internal data class SetupState(
    /** 0 arquivo, 1 senha de sincronização, 2 primeira sincronização. */
    val step: Int = 0,
    val fileName: String = "",
    val passphrase: String = "",
    val canRemember: Boolean = true,
    /** When [canRemember] is false: the Android is older than 9 (else the phone has no screen lock). */
    val oldAndroid: Boolean = false,
    val remember: Boolean = false,
    val syncing: Boolean = false,
    /** Result of the first sync (empty before it ran). */
    val status: String = "",
    val statusError: Boolean = false,
    /** The first sync worked. */
    val done: Boolean = false
)
