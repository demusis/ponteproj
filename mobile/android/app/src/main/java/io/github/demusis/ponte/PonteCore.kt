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
    /** Names used as responsible person, most used first: {names: [String]}. */
    external fun assignees(): String

    /**
     * Every atendimento, ordered for the list (open ones by urgency, closed ones last):
     * {open: N, attendances: [{id, reference, subject, status, status_label, open, due_date,
     * due_label, late, due_today, assignee, observation}]}. Read-only.
     */
    external fun listAttendances(): String

    /**
     * One atendimento to read: {id, reference, subject, status_label, open, due_label, late,
     * due_today, fields: [{label, value}], flags: [String], review_flags: [String], document_id,
     * document_number, document_title, tasks: [{id, title, done, due_label}], events: [{when,
     * actor, kind, detail}], attachments: [{id, name, kind, size, description}]}.
     */
    external fun getAttendance(attendanceId: Long): String

    /**
     * The resources register, by name: {resources: [{uid, name, kind, kind_label, quantity, location,
     * identification, notes, status, status_label, active}]}. Read-only.
     */
    external fun listResources(): String

    /**
     * Every use of a resource, by start: {uses: [{uid, resource_uid, resource_name, resource_kind,
     * resource_kind_label, quantity, who, sector, purpose, start, end (AAAA-MM-DDTHH:MM), returned_at
     * (null or text), status, status_label, notes, overdue, document_id, document_number,
     * document_title, attendance_id, attendance_reference, task_id, task_title}]} (links that do not
     * exist come as 0 and ""). The answer of [today] also carries "resources": {reservations_today, overdue}.
     */
    external fun listResourceUses(): String

    /**
     * The search over the whole library (two characters or more): {documents, tasks,
     * attendances}, each {rows: [...], more: N} already cut at eight.
     */
    external fun search(query: String): String
    external fun choices(): String
    external fun createPhotoRecord(fieldsJson: String, imagePath: String): String

    /**
     * A manual record from a file and/or a text: {kind, title, origin, date, text, file_name,
     * file_description}; filePath "" means no file (then the text is needed). The file is checked
     * before the record exists. Answer: {document_id}.
     */
    external fun createRecord(fieldsJson: String, filePath: String): String

    /**
     * Attaches the private working file at [path] to a document, with the desktop's own limits and
     * checks. Answer: {attachment_id, name, size}.
     */
    external fun attachFile(documentId: Long, path: String, name: String, description: String): String
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
