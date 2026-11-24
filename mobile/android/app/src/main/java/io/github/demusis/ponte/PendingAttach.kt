package io.github.demusis.ponte

import org.json.JSONObject
import java.io.File
import java.util.UUID

/** What proves the record of a ficha where it was read: its identity and the library space (both empty from an older core). */
internal data class AttachProof(val ident: String = "", val space: String = "")

/**
 * A photo or file taken for a record (`desenho-f3-v3-adendo.md` §10 rule 1), on disk BEFORE the core sees it: the
 * operation that makes the attach idempotent, the record by its number here, its identity and the library space the
 * person chose it in, and the staged copy. It goes when the core answered for it (see [PendingAttachRules]).
 */
internal data class PendingAttach(
    val operationId: String,
    val librarySpace: String,
    val docId: Long,
    val docIdent: String,
    val staged: String,
    val name: String,
    val description: String
) {
    fun toJson(): String = JSONObject()
        .put("operation_id", operationId).put("library_space", librarySpace).put("doc_id", docId)
        .put("doc_ident", docIdent).put("staged", staged).put("name", name).put("description", description)
        .toString()

    companion object {
        fun fromJson(text: String): PendingAttach? = try {
            val o = JSONObject(text)
            val p = PendingAttach(
                o.s("operation_id"), o.s("library_space"), o.optLong("doc_id"), o.s("doc_ident"),
                o.s("staged"), o.s("name"), o.s("description")
            )
            if (p.operationId.isEmpty() || p.docId <= 0 || p.staged.isEmpty()) null else p
        } catch (_: Throwable) {
            null
        }
    }
}

/** A file the core refused and keeps in card [cardId], waiting for the person (see `PonteCore.refusedAttachments`). */
internal data class RefusedFile(val cardId: Long, val name: String, val size: Long, val message: String)

internal object PendingAttachRules {
    /** The recovery screen's actions (frozen rule 22; P1 adopted). */
    val ACTIONS = listOf("Criar registro com esta foto", "Anexar a outro registro…", "Descartar")

    enum class Next {
        /** Not committed: hand it to the core again, with the same operation. */
        DELIVER,
        /** The core has it (attached, queued, or kept in a card): the staging and the record go. */
        CLEAN,
        /** Refused into a card: the staging and the record go, and the recovery screen opens. */
        RECOVER,
        /** No answer: kept for the next start, never guessed. */
        KEEP
    }

    /** The options of `attachFile` for [p]. */
    fun options(p: PendingAttach): String = JSONObject()
        .put("operation_id", p.operationId).put("document_ident", p.docIdent).put("library_space", p.librarySpace)
        .toString()

    /** What a record found at start becomes, from the core's `operationStatus` answer (null: none). */
    fun next(status: JSONObject?): Next {
        if (status == null || !status.optBoolean("ok") || !status.has("done")) return Next.KEEP
        if (!status.optBoolean("done")) return Next.DELIVER
        if (status.s("kind") != "attach") return Next.KEEP
        return if (status.s("outcome") == "refused") Next.RECOVER else Next.CLEAN
    }

    /** After `attachFile` answered ([error] null: success). Only an error without the core's answer keeps the staging. */
    fun afterAttach(error: Throwable?): Next = when {
        error == null -> Next.CLEAN
        error is CoreException && error.code == "target_gone" -> Next.RECOVER
        error is CoreException -> Next.CLEAN
        else -> Next.KEEP
    }

    fun refused(answer: JSONObject): List<RefusedFile> = answer.optJSONArray("items").objs()
        .map { RefusedFile(it.optLong("card_id"), it.s("name"), it.optLong("size"), it.s("message")) }
        .filter { it.cardId > 0 }

    /** The line that leads to the recovery screen ("" for none). */
    fun banner(count: Int): String = when {
        count <= 0 -> ""
        count == 1 -> "1 arquivo do celular não foi anexado"
        else -> "$count arquivos do celular não foram anexados"
    }
}

/** Writes [text] to [file] whole or not at all. */
private fun writeAtomically(file: File, text: String) {
    file.parentFile?.mkdirs()
    val tmp = File(file.parentFile, file.name + ".tmp")
    tmp.writeText(text)
    if (!tmp.renameTo(file)) {
        file.delete()
        if (!tmp.renameTo(file)) throw IllegalStateException("não foi possível gravar ${file.name}")
    }
}

/** The folder of the [PendingAttach] records and their staged copies, in the app's private files (never the cache). */
internal class PendingAttachStore(private val dir: File) {
    companion object {
        const val DIR = "anexos-pendentes"
    }

    /** [work] becomes the staged copy of a new record for [docId], written before it is answered. */
    fun adopt(work: File, docId: Long, proof: AttachProof, name: String, description: String): PendingAttach {
        dir.mkdirs()
        val op = UUID.randomUUID().toString()
        val staged = File(dir, "$op.bin")
        if (!work.renameTo(staged)) {
            work.copyTo(staged, overwrite = true)
            work.delete()
        }
        val p = PendingAttach(op, proof.space, docId, proof.ident, staged.absolutePath, name, description)
        try {
            writeAtomically(File(dir, "$op.json"), p.toJson())
        } catch (e: Throwable) {
            staged.delete()
            throw e
        }
        return p
    }

    fun all(): List<PendingAttach> = (dir.listFiles() ?: emptyArray())
        .filter { it.isFile && it.name.endsWith(".json") }
        .sortedBy { it.lastModified() }
        .mapNotNull { f -> try { PendingAttach.fromJson(f.readText()) } catch (_: Throwable) { null } }

    /** The record of [p] and its staged copy (never a file outside this folder). */
    fun remove(p: PendingAttach) {
        val staged = File(p.staged)
        if (staged.parentFile?.canonicalPath == dir.canonicalPath) staged.delete()
        File(dir, p.operationId + ".json").delete()
    }
}

/**
 * The one operation of each refused card (frozen rule 22): persisted before any recovery action calls the core, and the
 * same for every action on that card, so whatever the person does, and however often, the core makes it once.
 */
internal class RecoveryOps(private val file: File) {
    companion object {
        const val FILE = "recuperacao.json"
    }

    private fun read(): JSONObject = try { JSONObject(file.readText()) } catch (_: Throwable) { JSONObject() }

    @Synchronized fun opFor(cardId: Long): String {
        val o = read()
        o.s(cardId.toString()).takeIf { it.isNotEmpty() }?.let { return it }
        val op = UUID.randomUUID().toString()
        writeAtomically(file, o.put(cardId.toString(), op).toString())
        return op
    }

    /** Keeps only the operations of [cards] (the ones still waiting): any other card is done with. */
    @Synchronized fun keepOnly(cards: Set<Long>) {
        val o = read()
        val gone = o.keys().asSequence().toList().filter { it.toLongOrNull() !in cards }
        if (gone.isEmpty()) return
        gone.forEach { o.remove(it) }
        writeAtomically(file, o.toString())
    }

    @Synchronized fun forget(cardId: Long) {
        val o = read()
        if (o.has(cardId.toString())) {
            o.remove(cardId.toString())
            writeAtomically(file, o.toString())
        }
    }
}
