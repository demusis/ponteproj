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
    external fun listTaskRelations(): String
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
    external fun deleteTask(taskId: Long, version: Long): String
    external fun restoreTask(taskId: Long): String
    /** Every category, archived ones too: {categories: [{uid, name, color (0 to 9), archived}]}. */
    external fun listCategories(): String

    /** Creates a category (color is a palette index, 0 to 9). Answer: {category: {uid, name, color, archived}}. */
    external fun createCategory(name: String, color: Long): String

    /**
     * Replaces the categories of a document with [uidsJson] (a JSON array of uids, at most 10);
     * [version] is "categories_version" of getDocument, and a stale one saves nothing.
     * Answer: {categories, categories_version}.
     */
    external fun setDocumentCategories(documentId: Long, uidsJson: String, version: Long): String

    /**
     * Every project, by name, with its numbers: {projects: [{uid, name, objective, assignee,
     * status ("ativo", "pausado", "concluido", "arquivado"), status_label, start_date,
     * target_date (AAAA-MM-DD or ""), start_label, target_label, created_at, updated_at, version,
     * progress: {total, done, late, next_due (or null), next_due_label},
     * risk: {deliveries_at_risk, target_at_risk (AAAA-MM-DDTHH:MM or null)}}]}. Read-only.
     */
    external fun listProjects(): String

    /**
     * One project: {project (as in listProjects), tasks: [task] (the same task JSON as listTasks
     * with "project_uid" and "project_name"), timeline: [{id, uid, title, done, start, end
     * (AAAA-MM-DDTHH:MM or null), projected, due (AAAA-MM-DD or null), late}], span: {start, end}
     * or null}. Read-only.
     */
    external fun getProject(uid: String): String

    /**
     * Creates (no "uid") or changes a project; the fields sent among name, objective, assignee,
     * status, start_date, target_date are written, and the "version" of listProjects/getProject
     * is required for an existing one (a project changed meanwhile saves nothing).
     * Answer: {uid, version, project}.
     */
    external fun saveProject(json: String): String

    /** Deletes a project; its tasks stay with no project. Answer: {released: N tasks}. */
    external fun deleteProject(uid: String): String

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
     * The cobranças of today (open atendimentos whose follow-up is due today or before, most overdue
     * first): {count, channels: [String], follow_ups: [row of listAttendances + follow_up (why) +
     * follow_up_days (days overdue, 0 is today)]}.
     */
    external fun listFollowUps(): String

    /**
     * Registers a contact in the timeline: {"id", "contact" (empty: "Cobrado por telefone"),
     * "next_date" (AAAA-MM-DD, optional)}. The deadline of the atendimento never changes.
     */
    external fun registerFollowUp(json: String): String

    /** Only sets the next follow-up: {"id", "next_date" (AAAA-MM-DD, after today)}. */
    external fun scheduleFollowUp(json: String): String

    /**
     * The Recursos screens for a text (empty: all), decided by the core as of now: {uses: [...every use, with
     * period and overdue], resources: [...every resource, with in_use_now, free_now, usage_line, free_text,
     * today and next (uids of uses)], groups: [{key, title, late, uids}] of the uses that match, rows: [uids
     * of the resources that match]}. The answer of [today] also carries "resources": {reservations_today, overdue}.
     */
    external fun resourceView(query: String): String

    /** The ids, in order, of the documents that match a text, by the core's own search: {ids: [...]}. */
    external fun documentIds(query: String): String

    /**
     * The renumber notices still to be read, newest first: {notices: [{id, title, from, to, sentence, document_id
     * (0: cannot be opened), at}]}. They do not depend on the status line of a sync.
     */
    external fun renumberNotices(): String

    /** Marks a notice as read; an unknown or already read id is not an error. Answer: {remaining}. */
    external fun dismissRenumberNotice(id: Long): String

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
     *
     * An optional "operation_id" (a UUID) makes the operation idempotent, with or without a file: a second call
     * with the same id creates nothing and answers the same success (see [operationStatus]).
     */
    external fun createRecord(fieldsJson: String, filePath: String): String

    /**
     * Attaches the private working file at [path] to a document, with the desktop's own limits and
     * checks. Answer: {attachment_id, name, size}.
     *
     * [optionsJson] is a JSON object, "{}" when there is nothing to say, with two optional fields: "operation_id"
     * (a UUID; same guarantee as in [createRecord]) and "expected_number" (the number of the document the person
     * chose, e.g. "MANUAL-2026/0003"). A document that no longer exists, is in the trash or has another number
     * writes NOTHING and answers {"ok":false,"code":"target_changed","error":"..."}.
     */
    external fun attachFile(documentId: Long, path: String, name: String, description: String, optionsJson: String): String

    /**
     * Whether an operation was committed. Input {"operation_id":"<uuid>"}; answer {"ok":true,"done":true,
     * "document_id":N} when it was, {"ok":true,"done":false} when not. Read-only. The answer may also carry an
     * optional string "kind": "record" (a created record) or "attach" (a file attached to an existing document);
     * an older core leaves it out.
     */
    external fun operationStatus(json: String): String

    /**
     * F3.1b2: the files the core refused for a record and keeps in a card, `{items:[{operation_id, card_id, name,
     * size, message}]}`, and the three ways out of a card, each idempotent by the "operation_id" it carries: a new
     * record with the file (fields as in [createRecord]), the file attached to another record (options as in
     * [attachFile]), or the file dropped.
     */
    external fun refusedAttachments(): String
    external fun createRecordFromCard(cardId: Long, fieldsJson: String): String
    external fun attachFromCard(cardId: Long, documentId: Long, optionsJson: String): String
    external fun discardCard(cardId: Long): String
    external fun listMailAccounts(): String
    external fun saveMailAccount(json: String): String
    external fun deleteMailAccount(uid: String): String
    external fun listMailFolders(uid: String, password: String): String
    external fun listMailMessages(uid: String, password: String, folderRaw: String, filter: String): String
    external fun importMail(uid: String, password: String, folderRaw: String, uidsJson: String): String

    /** Deliveries at risk: {items:[{task_id,title,due_date,kind,end,cause_id,cause_title}]}. */
    external fun atRisk(): String

    /**
     * Saves the holidays chosen in Ajustes (the text of [HolidayConfig.toText]: a state, extra days off) in the
     * library, so every computer and phone that syncs it gets them, and makes them the planning's. Answers `text`.
     */
    external fun setHolidays(text: String): String

    /**
     * At startup: takes the holidays of the library (the planning of the core uses them from now on). When the
     * library has none yet, [seedText] (the copy this phone kept until now; empty when it was already handed
     * over) is put into it as a seed: applied only where there is no value, never over one, even a cleared one.
     * Answers `text`, the holidays in force ([localText] while the library has none), and `library`, whether
     * the library has a value.
     */
    external fun adoptHolidays(localText: String, seedText: String): String

    companion object {
        init {
            System.loadLibrary("ponte_mobile")
        }
    }
}
