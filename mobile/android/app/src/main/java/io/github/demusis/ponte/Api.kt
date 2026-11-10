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

    fun setDone(taskId: Long, done: Boolean) {
        c(if (done) core.completeTask(taskId) else core.reopenTask(taskId))
    }

    fun createTask(documentId: Long, title: String, action: String, due: String): Long {
        val o = JSONObject().put("document_id", documentId).put("title", title)
            .put("action", action).put("due_date", due)
        return c(core.createTask(o.toString())).optLong("id")
    }

    fun today(): JSONObject = c(core.today())

    fun task(id: Long): TaskDetail = c(core.getTask(id)).toTaskDetail()

    /** Sends only [changes] (field to value); [version] is the one the form started from. Answers the new version. */
    fun saveTask(id: Long, version: Long, changes: Map<String, String>): Long {
        val o = JSONObject().put("id", id).put("version", version)
        for ((k, v) in changes) o.put(k, v)
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
