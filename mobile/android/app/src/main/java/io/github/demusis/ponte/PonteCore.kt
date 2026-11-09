package io.github.demusis.ponte

/**
 * Bridge to the Rust core (libponte_mobile.so). Every function returns a JSON string:
 * {"ok":true,...} or {"ok":false,"error":"message"}.
 */
class PonteCore {
    external fun init(dataDir: String, libraryKey: ByteArray, memberName: String): String

    /**
     * pendingPath is "" when there is no pending copy.
     * Answer: {"ok":true,"written":bool,"summary":"...","waiting":N,"conflicts":N}
     */
    external fun sync(transportPath: String, pendingPath: String, passphrase: String): String

    /** May answer {"ok":false,"error":...}; a document may carry "note_error" instead of "note". */
    external fun listDocuments(): String
    external fun saveNote(documentId: Long, text: String): String

    companion object {
        init {
            System.loadLibrary("ponte_mobile")
        }
    }
}
