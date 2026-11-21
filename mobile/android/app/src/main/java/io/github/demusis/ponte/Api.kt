package io.github.demusis.ponte

import org.json.JSONArray
import org.json.JSONObject

/** String field that treats a missing key and JSON null as the default. */
internal fun JSONObject.s(key: String, def: String = ""): String =
    if (isNull(key)) def else optString(key, def)

internal fun JSONArray?.objs(): List<JSONObject> {
    if (this == null) return emptyList()
    return (0 until length()).mapNotNull { optJSONObject(it) }
}

/** Typed wrappers over the core. Blocking: call them from SyncRunner.background only. */
internal object Api {
    private val core get() = SyncRunner.core
    private fun c(json: String) = SyncRunner.check(json)

    @Volatile private var choicesCache: JSONObject? = null

    fun document(id: Long) = c(core.getDocument(id))

    fun exportFile(kind: String, id: Long, captureId: Long, destDir: String) =
        c(core.exportFile(kind, id, captureId, destDir))

    fun tasks(): List<JSONObject> = c(core.listTasks()).optJSONArray("tasks").objs()
    fun taskRelations(): List<JSONObject> = c(core.listTaskRelations()).optJSONArray("tasks").objs()

    fun setDone(taskId: Long, done: Boolean) {
        c(if (done) core.completeTask(taskId) else core.reopenTask(taskId))
    }

    fun createTask(
        documentId: Long, title: String, action: String, due: String, assignee: String = "",
        categories: List<String> = emptyList(), planning: NewPlanning = NewPlanning(), projectUid: String = ""
    ): Long {
        val o = JSONObject().put("document_id", documentId).put("title", title)
            .put("action", action).put("due_date", due).put("assignee", assignee)
            .put("categories", JSONArray(categories))
        // Absent leaves the task with no project (a subtask then takes its parent's).
        if (projectUid.isNotEmpty()) o.put("project_uid", projectUid)
        planning.fields().forEach { (k, v) -> o.put(k, if (v is List<*>) JSONArray(v) else v) }
        return c(core.createTask(o.toString())).optLong("id")
    }

    /** Saves the holidays chosen in Ajustes in the library (the other devices get them with the next sync). */
    fun setHolidays(text: String): String = c(core.setHolidays(text)).optString("text")

    /** Every category (archived ones too), by name. */
    fun categories(): List<Category> = c(core.listCategories()).optJSONArray("categories").objs().map { it.toCategory() }

    /** Creates a category and answers it as stored. */
    fun createCategory(name: String, color: Int): Category =
        c(core.createCategory(name, color.toLong())).getJSONObject("category").toCategory()

    /** Replaces the categories of a document; [version] is the one its ficha was read at. Answers the new version. */
    fun setDocumentCategories(documentId: Long, uids: List<String>, version: Long): Long =
        c(core.setDocumentCategories(documentId, JSONArray(uids).toString(), version)).optLong("categories_version")

    /** Every project, by name, with its progress and risk. */
    fun projects(): List<ProjectItem> = c(core.listProjects()).optJSONArray("projects").objs().map { it.toProject() }

    /** One project with its tasks and timeline. */
    fun project(uid: String): ProjectDetail = c(core.getProject(uid)).toProjectDetail()

    /** Creates or changes a project ([ProjectRules.saveJson]); answers the project as stored. */
    fun saveProject(json: JSONObject): ProjectItem = c(core.saveProject(json.toString())).getJSONObject("project").toProject()

    /** Deletes a project; answers how many tasks went back to no project. */
    fun deleteProject(uid: String): Int = c(core.deleteProject(uid)).optInt("released")

    fun assignees(): List<String> {
        val names = c(core.assignees()).optJSONArray("names") ?: return emptyList()
        return (0 until names.length()).map { names.optString(it) }.filter { it.isNotBlank() }
    }

    fun attendances(): List<AttendanceItem> = c(core.listAttendances()).optJSONArray("attendances").objs().map { it.toAttendance() }

    fun attendance(id: Long): AttendanceDetail = c(core.getAttendance(id)).toAttendanceDetail()

    /** The cobranças of today, and the contacts the computer offers for registering one. */
    fun followUps(): FollowUps {
        val o = c(core.listFollowUps())
        val channels = o.optJSONArray("channels")
        return FollowUps(
            rows = o.optJSONArray("follow_ups").objs().map { it.toAttendance() },
            channels = (0 until (channels?.length() ?: 0)).map { channels!!.optString(it) }.filter { it.isNotBlank() }
        )
    }

    /** A contact in the timeline, and maybe the next date ([nextDate] AAAA-MM-DD, or empty). */
    fun registerFollowUp(id: Long, contact: String, nextDate: String) {
        c(core.registerFollowUp(JSONObject().put("id", id).put("contact", contact).put("next_date", nextDate).toString()))
    }

    /** Only the next date, AAAA-MM-DD. */
    fun scheduleFollowUp(id: Long, nextDate: String) {
        c(core.scheduleFollowUp(JSONObject().put("id", id).put("next_date", nextDate).toString()))
    }

    fun resourceView(query: String): ResourcesState = c(core.resourceView(query)).toResourcesState()

    /** The ids of the documents the core's search finds for [query]. */
    fun documentIds(query: String): Set<Long> =
        c(core.documentIds(query)).optJSONArray("ids").let { a -> (0 until (a?.length() ?: 0)).map { a!!.optLong(it) }.toSet() }

    /** The query is passed on and forgotten: nothing here keeps or logs it. */
    fun search(query: String): SearchResult = c(core.search(query)).toSearchResult()

    fun today(): JSONObject = c(core.today())

    /** Deliveries at risk; an unknown kind is skipped. */
    fun atRisk(): List<RiskItem> = c(core.atRisk()).optJSONArray("items").objs().mapNotNull {
        val kind = it.s("kind")
        if (kind !in RiskRules.KINDS) null
        else RiskItem(
            taskId = it.optLong("task_id"), title = it.s("title"), dueDate = it.s("due_date"), kind = kind,
            end = it.s("end"), causeId = it.optLong("cause_id"), causeTitle = it.s("cause_title")
        )
    }

    fun task(id: Long): TaskDetail = c(core.getTask(id)).toTaskDetail()

    /** Sends only [changes] (field to value); [version] is the one the form started from. Answers the new version. */
    fun saveTask(id: Long, version: Long, changes: Map<String, Any>): Long {
        val o = JSONObject().put("id", id).put("version", version)
        for ((k, v) in changes) o.put(k, if (v is List<*>) JSONArray(v) else v)
        return c(core.saveTask(o.toString())).optLong("version")
    }

    /** Answers the new version. */
    fun relinkTask(id: Long, documentId: Long, version: Long): Long =
        c(core.relinkTask(id, documentId, version)).optLong("version")

    fun choices(): JSONObject {
        choicesCache?.let { return it }
        val o = c(core.choices())
        choicesCache = o
        return o
    }

    fun createPhoto(fields: String, imagePath: String) = c(core.createPhotoRecord(fields, imagePath))

    /** Answers the new document's id. [filePath] is "" for a record of text only. */
    fun createRecord(fields: String, filePath: String): Long = c(core.createRecord(fields, filePath)).optLong("document_id")

    /**
     * [operationId] (a UUID) makes the attach idempotent; [expectedNumber] is the number of the document the person
     * chose: the core writes nothing, and answers the code "target_changed", when the document is not that one anymore.
     * Both are sent only when given.
     */
    fun attachFile(
        documentId: Long, path: String, name: String, description: String,
        operationId: String = "", expectedNumber: String = ""
    ) {
        val options = JSONObject()
        if (operationId.isNotEmpty()) options.put("operation_id", operationId)
        if (expectedNumber.isNotEmpty()) options.put("expected_number", expectedNumber)
        c(core.attachFile(documentId, path, name, description, options.toString()))
    }

    /**
     * What the core says about the operation [operationId]: committed or not. Anything else (an error, an answer
     * without "done", an older core that does not have the call) is "unknown": never thrown, never guessed.
     * [expectedKind] is the kind of the marker asking ("record" or "attach"): an answer whose optional "kind" says
     * another is "unknown" too (see [ShareDraftRules.statusFrom]).
     */
    fun operationStatus(operationId: String, expectedKind: String? = null): ShareDraftRules.OperationStatus = try {
        val o = JSONObject(core.operationStatus(JSONObject().put("operation_id", operationId).toString()))
        val done = if (o.has("done") && !o.isNull("done")) o.optBoolean("done") else null
        val kind = if (o.has("kind") && !o.isNull("kind")) o.optString("kind") else null
        ShareDraftRules.statusFrom(o.optBoolean("ok"), done, o.optLong("document_id"), kind, expectedKind)
    } catch (_: Throwable) {
        ShareDraftRules.OperationStatus.Unknown
    }

    fun mailAccounts(): List<JSONObject> = c(core.listMailAccounts()).optJSONArray("accounts").objs()
    fun saveMailAccount(json: String): String = c(core.saveMailAccount(json)).s("uid")
    fun deleteMailAccount(uid: String) { c(core.deleteMailAccount(uid)) }
    fun mailFolders(uid: String, pw: String): List<JSONObject> =
        c(core.listMailFolders(uid, pw)).optJSONArray("folders").objs()

    fun mailMessages(uid: String, pw: String, folder: String, filter: String): List<JSONObject> =
        c(core.listMailMessages(uid, pw, folder, filter)).optJSONArray("messages").objs()

    fun importMail(uid: String, pw: String, folder: String, uidsJson: String): List<JSONObject> =
        c(core.importMail(uid, pw, folder, uidsJson)).optJSONArray("results").objs()
}
