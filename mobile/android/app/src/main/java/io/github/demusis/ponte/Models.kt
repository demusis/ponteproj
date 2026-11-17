package io.github.demusis.ponte

import androidx.compose.ui.graphics.ImageBitmap
import org.json.JSONObject
import java.time.LocalDate

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
    val notes: String = "",
    /** Uids of the categories (names and colours come from [Category]). */
    val categories: List<String> = emptyList(),
    /** The core's deadline group: "atrasadas", "hoje", "proximas", "sem_prazo" or "concluidas". */
    val bucket: String = "",
    /** What the task would be if it were open: late, or due today (the core decides; used when it is reopened). */
    val lateIfOpen: Boolean = false,
    val todayIfOpen: Boolean = false,
    val uid: String = "",
    val scheduledStart: String = "",
    val scheduledEnd: String = "",
    val estimateMinutes: Int = 0,
    val parentTaskId: Long? = null,
    val reminderOffsetsMinutes: List<Int> = emptyList(),
    val dependencies: List<Long> = emptyList()
) {
    /** The group while open: the core's, or, for a task reopened here and not yet reloaded, from its flags. */
    val openBucket: String
        get() = when {
            bucket != "concluidas" -> bucket
            lateIfOpen -> "atrasadas"
            todayIfOpen -> "hoje"
            dueDate.isEmpty() -> "sem_prazo"
            else -> "proximas"
        }

    /** The second line of a row: the action and the document ("Sem documento" for none). */
    val secondLine: String
        get() = listOf(actionLabel, if (documentId <= 0) "Sem documento" else documentNumber)
            .filter { it.isNotEmpty() }.joinToString(" · ")
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
        dueChip = s("due_chip"),
        done = optBoolean("done"),
        overdue = optBoolean("overdue"),
        dueToday = optBoolean("due_today"),
        priority = s("priority", "normal").ifEmpty { "normal" },
        priorityLabel = s("priority_label"),
        notes = s("notes"),
        categories = optJSONArray("categories").strings(),
        bucket = s("bucket"),
        lateIfOpen = optBoolean("late_if_open"),
        todayIfOpen = optBoolean("today_if_open"),
        uid = s("uid"), scheduledStart = s("scheduled_start"), scheduledEnd = s("scheduled_end"),
        estimateMinutes = optInt("estimate_minutes"),
        parentTaskId = if (isNull("parent_task_id")) null else optLong("parent_task_id"),
        dependencies = optJSONArray("dependencies")?.let { a -> (0 until a.length()).map { a.optLong(it) } } ?: emptyList(),
        reminderOffsetsMinutes = optJSONArray("reminder_offsets_minutes")?.let { a -> (0 until a.length()).map { a.optInt(it) } } ?: emptyList()
    )
}

/** An atendimento in a list (Hoje, Atendimentos, the search), read-only on the phone. */
internal data class AttendanceItem(
    val id: Long,
    /** Reference, or the subject when there is none. */
    val reference: String,
    val subject: String,
    val statusLabel: String,
    val dueDate: String,
    val dueChip: String,
    val late: Boolean,
    /** Responsible person; may be empty. */
    val assignee: String = "",
    /** Only the Atendimentos list carries it (for its search). */
    val observation: String = "",
    /** Not encerrado nor cancelado. */
    val open: Boolean = true,
    val dueToday: Boolean = false
)

internal fun JSONObject.toAttendance() = AttendanceItem(
    id = optLong("id"), reference = s("reference"), subject = s("subject"),
    statusLabel = s("status_label"), dueDate = s("due_date"),
    dueChip = s("due_chip"), late = optBoolean("late"),
    assignee = s("assignee"), observation = s("observation"), open = optBoolean("open", true),
    dueToday = optBoolean("due_today")
)

/** A fact of an atendimento's ficha: the desktop's label and its text. */
internal data class AttendanceField(val label: String, val value: String)

internal data class AttendanceTask(val id: Long, val title: String, val done: Boolean, val dueLabel: String)

/** One line of the atendimento's timeline, newest first. */
internal data class AttendanceEvent(val whenText: String, val actor: String, val kind: String, val detail: String)

/** Everything the atendimento's ficha shows (nothing on it can be changed on the phone). */
internal data class AttendanceDetail(
    val id: Long,
    val reference: String,
    val subject: String,
    val statusLabel: String,
    val open: Boolean,
    /** dd/mm/aaaa, or empty. */
    val dueLabel: String,
    val late: Boolean,
    val dueToday: Boolean,
    val fields: List<AttendanceField>,
    /** "Agendado", "Documento sensível". */
    val flags: List<String>,
    val reviewFlags: List<String>,
    /** 0: no document linked. */
    val documentId: Long,
    val documentNumber: String,
    val documentTitle: String,
    val tasks: List<AttendanceTask>,
    val events: List<AttendanceEvent>,
    val attachments: List<FileItem>
) {
    /** "ORG-… — assunto", or whichever of the two exists. */
    val documentLine: String
        get() = listOf(documentNumber, documentTitle).filter { it.isNotBlank() }.joinToString(" — ")
}

internal fun JSONObject.toAttendanceDetail() = AttendanceDetail(
    id = optLong("id"), reference = s("reference"), subject = s("subject"), statusLabel = s("status_label"),
    open = optBoolean("open", true), dueLabel = s("due_label"), late = optBoolean("late"),
    dueToday = optBoolean("due_today"),
    fields = optJSONArray("fields").objs().map { AttendanceField(it.s("label"), it.s("value")) },
    flags = optJSONArray("flags").strings(), reviewFlags = optJSONArray("review_flags").strings(),
    documentId = optLong("document_id"), documentNumber = s("document_number"), documentTitle = s("document_title"),
    tasks = optJSONArray("tasks").objs().map { AttendanceTask(it.optLong("id"), it.s("title"), it.optBoolean("done"), it.s("due_label")) },
    events = optJSONArray("events").objs().map { AttendanceEvent(it.s("when"), it.s("actor"), it.s("kind"), it.s("detail")) },
    attachments = optJSONArray("attachments").objs().map {
        FileItem("attachment", it.optLong("id"), 0L, it.s("name").ifEmpty { "Anexo" }, it.optLong("size"), it.s("description"))
    }
)

internal fun org.json.JSONArray?.strings(): List<String> {
    if (this == null) return emptyList()
    return (0 until length()).map { optString(it) }.filter { it.isNotBlank() }
}

/** A document found by the search. */
internal data class SearchDoc(val id: Long, val number: String, val title: String, /** "Manual", "E-mail" or empty. */ val kind: String)

/** At most eight rows of a group and how many more there are. */
internal data class SearchGroup<T>(val rows: List<T>, val more: Int) {
    companion object {
        fun <T> none() = SearchGroup<T>(emptyList(), 0)
    }
}

internal data class SearchResult(
    val documents: SearchGroup<SearchDoc>,
    val tasks: SearchGroup<TaskItem>,
    val attendances: SearchGroup<AttendanceItem>
) {
    val total: Int get() = documents.rows.size + tasks.rows.size + attendances.rows.size
}

internal fun JSONObject.toSearchResult(): SearchResult {
    fun <T> group(key: String, row: (JSONObject) -> T): SearchGroup<T> {
        val g = optJSONObject(key) ?: return SearchGroup.none()
        return SearchGroup(g.optJSONArray("rows").objs().map(row), g.optInt("more"))
    }
    return SearchResult(
        documents = group("documents") { SearchDoc(it.optLong("id"), it.s("number"), it.s("title"), it.s("kind")) },
        tasks = group("tasks") { it.toTask() },
        attendances = group("attendances") { it.toAttendance() }
    )
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
    val notes: String,
    /** Uids of the categories, in the order they were chosen. */
    val categories: List<String> = emptyList(),
    val scheduledStart: String = "",
    val scheduledEnd: String = "",
    val estimateMinutes: Int = 0,
    val parentTaskId: Long? = null,
    val reminderOffsetsMinutes: List<Int> = emptyList()
) {
    companion object

    /** Whether the person changed something compared with [saved] (blanks around names do not count). */
    fun changedFrom(saved: TaskForm): Boolean = changes(saved).isNotEmpty()

    /** Field name (as the core's `saveTask` reads it) to new value, only for what differs. */
    fun changes(saved: TaskForm): Map<String, Any> {
        val out = LinkedHashMap<String, Any>()
        if (title.trim() != saved.title.trim()) out["title"] = title.trim()
        if (action != saved.action) out["action"] = action
        if (priority != saved.priority) out["priority"] = priority
        if (dueDate != saved.dueDate) out["due_date"] = dueDate
        if (scheduledStart != saved.scheduledStart) out["scheduled_start"] = scheduledStart
        if (scheduledEnd != saved.scheduledEnd) out["scheduled_end"] = scheduledEnd
        if (estimateMinutes != saved.estimateMinutes) out["estimate_minutes"] = estimateMinutes
        if (reminderOffsetsMinutes != saved.reminderOffsetsMinutes) out["reminder_offsets_minutes"] = reminderOffsetsMinutes
        if (parentTaskId != saved.parentTaskId) out["parent_task_id"] = parentTaskId ?: JSONObject.NULL
        if (assignee.trim() != saved.assignee.trim()) out["assignee"] = assignee.trim()
        if (notes != saved.notes) out["notes"] = notes
        if (categories.toSet() != saved.categories.toSet()) out["categories"] = categories
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
    val checklist: List<ChecklistItem>,
    val uid: String = "",
    val dependencies: List<Long> = emptyList()
) {
    val scheduledStart: String get() = form.scheduledStart
    val scheduledEnd: String get() = form.scheduledEnd
    val estimateMinutes: Int get() = form.estimateMinutes
    val parentTaskId: Long? get() = form.parentTaskId
    val reminderOffsetsMinutes: List<Int> get() = form.reminderOffsetsMinutes

    /** "ORG-… — assunto", or whichever of the two exists. */
    val documentLine: String
        get() = listOf(documentNumber, documentTitle).filter { it.isNotBlank() }.joinToString(" — ")

    /** As a row of a list, for the common actions (conclude, reopen). */
    fun asItem() = TaskItem(
        id = id, documentId = documentId, title = form.title, actionLabel = "", documentNumber = documentNumber,
        dueDate = form.dueDate, dueChip = "", done = done, overdue = false, dueToday = false,
        uid = uid, scheduledStart = form.scheduledStart, scheduledEnd = form.scheduledEnd,
        estimateMinutes = form.estimateMinutes, parentTaskId = form.parentTaskId, reminderOffsetsMinutes = form.reminderOffsetsMinutes
    )
}

internal fun JSONObject.toTaskDetail() = TaskDetail(
    id = optLong("id"), documentId = optLong("document_id"), documentNumber = s("document_number"),
    documentTitle = s("document_title"), uid = s("uid"),
    dependencies = optJSONArray("dependencies")?.let { a -> (0 until a.length()).map { a.optLong(it) } } ?: emptyList(),
    form = TaskForm(
        title = s("title"), action = s("action"), priority = s("priority", "normal").ifEmpty { "normal" },
        dueDate = s("due_date"), assignee = s("assignee"), notes = s("notes"),
        categories = optJSONArray("categories").strings(),
        scheduledStart = s("scheduled_start"), scheduledEnd = s("scheduled_end"),
        estimateMinutes = optInt("estimate_minutes"),
        parentTaskId = if (isNull("parent_task_id")) null else optLong("parent_task_id"),
        reminderOffsetsMinutes = optJSONArray("reminder_offsets_minutes")?.let { a -> (0 until a.length()).map { a.optInt(it) } } ?: emptyList()
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
    val text: String,
    /** Uids of the categories of the document. */
    val categories: List<String> = emptyList(),
    /** The version a change of the categories must present back (the core refuses a stale one). */
    val categoriesVersion: Long = 0
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
        text = s("text"),
        categories = optJSONArray("categories").strings(),
        categoriesVersion = optLong("categories_version")
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
    val syncEnabled: Boolean = true,
    /** The Tarefas area shows its Atendimentos list (the title says so). */
    val attendances: Boolean = false,
    /** The Tarefas area shows its Recursos list (the title says so). */
    val resources: Boolean = false
)

internal data class TasksState(
    val loading: Boolean = true,
    val error: String? = null,
    val tasks: List<TaskItem> = emptyList()
)

/** The Atendimentos list of the Tarefas area, ordered by the core (open first, closed last). */
internal data class AttendancesState(
    val loading: Boolean = true,
    val error: String? = null,
    val rows: List<AttendanceItem> = emptyList()
)

/** The atendimento screen: the atendimento as stored. [id] is the one the screen was opened for. */
internal data class AttendanceViewState(
    val id: Long = 0,
    val loading: Boolean = true,
    val error: String? = null,
    val detail: AttendanceDetail? = null
)

/**
 * A resource of the register (room, equipment, book...), read-only on the phone. [quantity] is how many
 * there are of it (the "Notebook de apresentações" may be 2).
 */
internal data class ResourceItem(
    val uid: String,
    val name: String,
    val quantity: Int,
    /** "Onde fica". */
    val location: String,
    /** Tombo, patrimônio, ISBN... */
    val identification: String,
    val notes: String,
    /** "ativo" or "fora_de_uso". */
    val status: String,
    val statusLabel: String,
    val active: Boolean,
    /** How many are out now, how many are free, and the line "1 de 3 em uso agora" (the core decides). */
    val inUseNow: Int = 0,
    val freeNow: Int = 0,
    val usageLine: String = "",
    /** "1 livre", "3 livres". */
    val freeText: String = "",
    /** Uids of the uses holding it today (by start) and of what is out or still ahead (by start). */
    val todayUids: List<String> = emptyList(),
    val nextUids: List<String> = emptyList()
)

internal fun JSONObject.toResource() = ResourceItem(
    uid = s("uid"), name = s("name"),
    quantity = optInt("quantity", 1), location = s("location"), identification = s("identification"),
    notes = s("notes"), status = s("status", "ativo"), statusLabel = s("status_label"),
    active = optBoolean("active", true),
    inUseNow = optInt("in_use_now"), freeNow = optInt("free_now"), usageLine = s("usage_line"), freeText = s("free_text"),
    todayUids = optJSONArray("today").strings(), nextUids = optJSONArray("next").strings()
)

/** One use of a resource: a reservation or a loan, with its start and end (`AAAA-MM-DDTHH:MM`, local time). */
internal data class ResourceUseItem(
    val uid: String,
    val resourceUid: String,
    val resourceName: String,
    val quantity: Int,
    /** "Quem". */
    val who: String,
    val sector: String,
    /** "Finalidade". */
    val purpose: String,
    val start: String,
    /** "Fim previsto". */
    val end: String,
    /** "Devolvido em"; empty while not returned. */
    val returnedAt: String,
    /** "reservado", "em_uso", "devolvido" or "cancelado". */
    val status: String,
    val statusLabel: String,
    val notes: String,
    /** In use and past its expected end, as the core decided when the screen was read. */
    val overdue: Boolean,
    /** "10/11 14:00 – 16:00", as the core words the period. */
    val period: String = "",
    /** 0: no document linked. */
    val documentId: Long = 0,
    val documentNumber: String = "",
    val documentTitle: String = "",
    val attendanceId: Long = 0,
    val attendanceReference: String = "",
    val taskId: Long = 0,
    val taskTitle: String = ""
) {
    /** "ORG-… — assunto", or whichever of the two exists. */
    val documentLine: String
        get() = listOf(documentNumber, documentTitle).filter { it.isNotBlank() }.joinToString(" — ")
}

internal fun JSONObject.toResourceUse() = ResourceUseItem(
    uid = s("uid"), resourceUid = s("resource_uid"), resourceName = s("resource_name"),
    quantity = optInt("quantity", 1),
    who = s("who"), sector = s("sector"), purpose = s("purpose"), start = s("start"), end = s("end"),
    returnedAt = s("returned_at"), status = s("status", "reservado"), statusLabel = s("status_label"),
    notes = s("notes"), overdue = optBoolean("overdue"), period = s("period"),
    documentId = optLong("document_id"), documentNumber = s("document_number"), documentTitle = s("document_title"),
    attendanceId = optLong("attendance_id"), attendanceReference = s("attendance_reference"),
    taskId = optLong("task_id"), taskTitle = s("task_title")
)

/** A group of the list of uses, as the core decided it (title included): the uses are those of [uids]. */
internal data class ResourceGroup(val key: String, val title: String, val late: Boolean, val uids: List<String>)

/**
 * The Recursos area as the core decided it for the text searched: all the uses and resources (the detail
 * screens look them up), the groups of the uses that match, and the uids of the resources that match.
 */
internal data class ResourcesState(
    val loading: Boolean = true,
    val error: String? = null,
    val uses: List<ResourceUseItem> = emptyList(),
    val resources: List<ResourceItem> = emptyList(),
    val groups: List<ResourceGroup> = emptyList(),
    val rows: List<String> = emptyList()
) {
    fun use(uid: String): ResourceUseItem? = uses.firstOrNull { it.uid == uid }

    fun usesOf(uids: List<String>): List<ResourceUseItem> {
        val byUid = uses.associateBy { it.uid }
        return uids.mapNotNull { byUid[it] }
    }

    /** The resources that match the text, as the core ordered them. */
    val rowResources: List<ResourceItem>
        get() {
            val byUid = resources.associateBy { it.uid }
            return rows.mapNotNull { byUid[it] }
        }
}

internal fun JSONObject.toResourcesState() = ResourcesState(
    loading = false,
    uses = optJSONArray("uses").objs().map { it.toResourceUse() },
    resources = optJSONArray("resources").objs().map { it.toResource() },
    groups = optJSONArray("groups").objs().map {
        ResourceGroup(it.s("key"), it.s("title"), it.optBoolean("late"), it.optJSONArray("uids").strings())
    },
    rows = optJSONArray("rows").strings()
)

/**
 * The search screen. The text lives in memory only (never in the saved state, never logged) and is
 * dropped when the screen closes or the app locks.
 */
internal data class SearchState(
    val query: String = "",
    /** A search for [query] is waiting or running. */
    val loading: Boolean = false,
    val error: String? = null,
    /** The answer for [query]; null while there is none (short text, or not answered yet). */
    val result: SearchResult? = null
)

/** What the Hoje area lists, already ordered by the core. */
internal data class TodayState(
    val loading: Boolean = true,
    val error: String? = null,
    val late: List<TaskItem> = emptyList(),
    val today: List<TaskItem> = emptyList(),
    val upcoming: List<TaskItem> = emptyList(),
    val attendances: List<AttendanceItem> = emptyList(),
    /** Resources reserved or in use today (a count; the names are in the Recursos list). */
    val reservationsToday: Int = 0,
    /** Resources in use past their expected end. */
    val overdueReturns: Int = 0,
    /** The core's line for the top of Hoje, and the number on the Tarefas tab. */
    val summary: String = "",
    val badge: Int = 0
) {
    val isEmpty: Boolean
        get() = late.isEmpty() && today.isEmpty() && upcoming.isEmpty() && attendances.isEmpty() &&
            reservationsToday == 0 && overdueReturns == 0
}

/** The task screen: the task as stored, and what is going on with it. */
internal data class TaskEditState(
    /** The task this state is about (the one the screen was opened for). */
    val id: Long = 0,
    val loading: Boolean = true,
    /** Why the task could not be read. */
    val error: String? = null,
    val task: TaskDetail? = null,
    /** Saving, concluding or changing the document: buttons wait. */
    val busy: Boolean = false,
    /** Shown above the buttons (a failed save, a changed task). */
    val formError: String = "",
    /** [formError] says the task changed since it was opened (offers "Ver a versão atual"). */
    val conflict: Boolean = false,
    /** Grows when the person asks for the current version: the form is dropped and starts over. */
    val epoch: Int = 0
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
    val secureScreen: Boolean = true,
    /** What the person typed as "Seu nome nas tarefas" (saved trimmed). */
    val personName: String = "",
    /** The daily notice of deadlines is on. */
    val noticeOn: Boolean = false,
    /** [DeadlineNotice.Time.key] of the chosen time. */
    val noticeTime: String = DeadlineNotice.DEFAULT.key,
    /** The notice is on or was asked for, but the phone does not let the app show notifications. */
    val noticeBlocked: Boolean = false
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
    val done: Boolean = false,
    /** "Seu nome nas tarefas", asked after the first sync (optional; saved as it is typed). */
    val personName: String = "",
    /** The first sync did not run because there is no Wi-Fi: the screen offers mobile data. */
    val noWifi: Boolean = false
)

/** What the first-sync step says after an attempt that did not finish. */
internal object SetupRules {
    const val NO_WIFI_PREFIX = "Sem Wi-Fi"

    /**
     * The reason to show when the sync is not running and did not work: any status text
     * (no Wi-Fi, fingerprint not released, password missing, an error), not only errors.
     * Empty when there is nothing to say (still running, done, or no attempt yet).
     */
    fun reason(busy: Boolean, text: String): String =
        if (busy || text.isEmpty() || text.startsWith("Sincronizado") || text == "Pronto.") "" else text

    fun noWifi(busy: Boolean, text: String): Boolean = !busy && text.startsWith(NO_WIFI_PREFIX)
}
