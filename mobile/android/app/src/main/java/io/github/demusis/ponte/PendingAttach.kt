package io.github.demusis.ponte

import java.io.File

// F3.1b2-ii, tests first: the shapes the tests name, nothing behind them yet.

internal data class AttachProof(val ident: String = "", val space: String = "")

internal data class PendingAttach(
    val operationId: String,
    val librarySpace: String,
    val docId: Long,
    val docIdent: String,
    val staged: String,
    val name: String,
    val description: String
)

internal data class RefusedFile(val cardId: Long, val name: String, val size: Long, val message: String)

internal object PendingAttachRules {
    val ACTIONS: List<String> = emptyList()

    enum class Next { DELIVER, CLEAN, RECOVER, KEEP }

    fun options(p: PendingAttach): String = TODO("F3.1b2-ii")
    fun next(status: org.json.JSONObject?): Next = TODO("F3.1b2-ii")
    fun afterAttach(error: Throwable?): Next = TODO("F3.1b2-ii")
    fun refused(answer: org.json.JSONObject): List<RefusedFile> = TODO("F3.1b2-ii")
    fun banner(count: Int): String = TODO("F3.1b2-ii")
}

internal class PendingAttachStore(private val dir: File) {
    companion object {
        const val DIR = "anexos-pendentes"
    }

    fun adopt(work: File, docId: Long, proof: AttachProof, name: String, description: String): PendingAttach = TODO("F3.1b2-ii")
    fun all(): List<PendingAttach> = TODO("F3.1b2-ii")
    fun remove(p: PendingAttach): Unit = TODO("F3.1b2-ii")
}

internal class RecoveryOps(private val file: File) {
    companion object {
        const val FILE = "recuperacao.json"
    }

    fun opFor(cardId: Long): String = TODO("F3.1b2-ii")
    fun forget(cardId: Long): Unit = TODO("F3.1b2-ii")
}
