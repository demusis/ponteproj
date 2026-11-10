package io.github.demusis.ponte

/**
 * Bridge to the Rust core (libponte_mobile.so). Every function returns a JSON string:
 * {"ok":true,...} or {"ok":false,"error":"message"}.
 */
class PonteCore {
    external fun init(dataDir: String, libraryKey: ByteArray, memberName: String): String

    /**
     * pendingPath is "" when there is no pending copy.
     * transportPath is the private working copy: the core rewrites it in place (atomically) when it changed.
     * Answer: {"ok":true,"written":bool,"bytes":N (only when written),"summary":"...","waiting":N,"conflicts":N}
     */
    external fun sync(transportPath: String, pendingPath: String, passphrase: String): String

    /** May answer {"ok":false,"error":...}; a document may carry "note_error" instead of "note". */
    external fun listDocuments(): String
    external fun saveNote(documentId: Long, text: String): String

    external fun getDocument(documentId: Long): String

    /** kind is "pdf" or "attachment"; captureId is 0 for attachments. Answer: {path,name,mime}. */
    external fun exportFile(kind: String, id: Long, captureId: Long, destDir: String): String
    external fun listTasks(): String
    external fun completeTask(taskId: Long): String
    external fun reopenTask(taskId: Long): String
    /** A missing or zero "document_id" makes a task without document. Answer: {id}. */
    external fun createTask(json: String): String

    /**
     * What Hoje lists: {late, today, upcoming: [task], attendances: [{id, reference, subject,
     * status_label, due_date, late}]}, already ordered.
     */
    external fun today(): String

    /** One task with every editable field, "checklist", "document_title" and its "version". */
    external fun getTask(taskId: Long): String

    /**
     * Saves the fields sent among title, action, priority, due_date, assignee, notes; "id" and the
     * "version" read by getTask are required. A task changed meanwhile answers an error and saves
     * nothing. Answer: {id, version}.
     */
    external fun saveTask(json: String): String

    /** Links, moves (documentId > 0) or unlinks (0) a task; version as in saveTask. */
    external fun relinkTask(taskId: Long, documentId: Long, version: Long): String
    external fun choices(): String
    external fun createPhotoRecord(fieldsJson: String, imagePath: String): String
    external fun listMailAccounts(): String
    external fun saveMailAccount(json: String): String
    external fun deleteMailAccount(uid: String): String
    external fun listMailFolders(uid: String, password: String): String
    external fun listMailMessages(uid: String, password: String, folderRaw: String, filter: String): String
    external fun importMail(uid: String, password: String, folderRaw: String, uidsJson: String): String

    companion object {
        init {
            System.loadLibrary("ponte_mobile")
        }
    }
}
