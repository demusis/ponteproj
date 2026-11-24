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
 *
 * [refusal]: the core refused the file itself when a start delivered it; its bytes are gone, and this record stays only
 * so Hoje tells the person why, until "Entendi" (`revisao-f3-1b2.md` finding 1).
 */
internal data class PendingAttach(
    val operationId: String,
    val librarySpace: String,
    val docId: Long,
    val docIdent: String,
    val staged: String,
    val name: String,
    val description: String,
    val refusal: String = ""
) {
    fun toJson(): String = JSONObject()
        .put("operation_id", operationId).put("library_space", librarySpace).put("doc_id", docId)
        .put("doc_ident", docIdent).put("staged", staged).put("name", name).put("description", description)
        .apply { if (refusal.isNotEmpty()) put("refusal", refusal) }
        .toString()

    companion object {
        fun fromJson(text: String): PendingAttach? = try {
            val o = JSONObject(text)
            val p = PendingAttach(
                o.s("operation_id"), o.s("library_space"), o.optLong("doc_id"), o.s("doc_ident"),
                o.s("staged"), o.s("name"), o.s("description"), o.s("refusal")
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

    /** Hoje's actions on a file the core did not take yet ([PendingNotice.canRetry]) and on one it refused. */
    const val RETRY = "Tentar de novo"
    const val DISCARD = "Descartar"
    const val UNDERSTOOD = "Entendi"

    enum class Next {
        /** Not committed: hand it to the core again, with the same operation. */
        DELIVER,
        /** The core has it (attached or queued, or its card already dealt with): the staging and the record go. */
        CLEAN,
        /** Kept in card N: the staging and the record go, and the recovery card of Hoje shows it. */
        RECOVER,
        /** The core refused the file itself (no record would take it): the staging goes, and the person is told. */
        REFUSED,
        /** Anything else: kept for "Tentar de novo" and the next start, never guessed, and the person is told. */
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
        if (status.s("outcome") != "refused") return Next.CLEAN
        return if (status.optLong("card_id") > 0) Next.RECOVER else Next.KEEP
    }

    /**
     * After `attachFile` answered ([error] null: success), `revisao-f3-1b2.md` finding 1: the staging goes only for an
     * answer that says where the file is. Attached; kept in a card (`target_gone` WITH its card); or refused for what
     * the file itself is (`file_refused`). Every other error (disk full, a panic, the library lock, a text without a
     * code, a code without its card, no answer at all) keeps it.
     */
    fun afterAttach(error: Throwable?): Next = when {
        error == null -> Next.CLEAN
        error is CoreException && error.code == "target_gone" && error.cardId > 0 -> Next.RECOVER
        error is CoreException && error.code == "file_refused" -> Next.REFUSED
        else -> Next.KEEP
    }

    /** What Hoje (and the screen, at attach time) says of a file kept on the phone because the core did not take it. */
    fun keptMessage(reason: String): String {
        val r = reason.trim().removeSuffix(".")
        val why = if (r.isEmpty()) "Não foi possível anexar agora." else "Não foi possível anexar agora: $r."
        return "$why O arquivo continua guardado no aparelho e pode ser enviado de novo em Hoje."
    }

    /** The header of Hoje's card of the files kept on the phone ("" for none). */
    fun noticeBanner(count: Int): String = when {
        count <= 0 -> ""
        count == 1 -> "1 arquivo guardado no celular ainda não foi anexado"
        else -> "$count arquivos guardados no celular ainda não foram anexados"
    }

    /** "Ver em Hoje" may take the person there: never over something being typed (finding 4, as [NavGuard]). */
    fun seeInToday(stack: List<Route>, attachBusy: Boolean): Boolean = !NavGuard.busy(stack, attachBusy)

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

        /**
         * A staged copy with no record is swept only once it is this old: [adopt] writes the copy, then the record,
         * moments apart, and never on this age (finding 5).
         */
        const val ORPHAN_MARGIN_MS = 10L * 60 * 1000
    }

    /**
     * [work] becomes the staged copy of a new record for [docId], written before it is answered. It may copy up to
     * the size cap: never on the main thread.
     */
    fun adopt(work: File, docId: Long, proof: AttachProof, name: String, description: String): PendingAttach {
        dir.mkdirs()
        val op = UUID.randomUUID().toString()
        val staged = File(dir, "$op.bin")
        if (!work.renameTo(staged)) {
            work.copyTo(staged, overwrite = true)
            work.delete()
        }
        // A new copy, whatever the age of the file it came from (see [sweepOrphans]).
        staged.setLastModified(System.currentTimeMillis())
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
        deleteStaged(p)
        File(dir, p.operationId + ".json").delete()
    }

    private fun deleteStaged(p: PendingAttach) {
        val staged = File(p.staged)
        if (staged.parentFile?.canonicalPath == dir.canonicalPath) staged.delete()
    }

    /** The core refused the file of [p] itself: the record says why (written first), and its bytes go. */
    fun markRefused(p: PendingAttach, message: String) {
        writeAtomically(File(dir, p.operationId + ".json"), p.copy(refusal = message).toJson())
        deleteStaged(p)
    }

    /**
     * Finding 5: deletes the staged copies no record leads to (the process died between the copy and its record) and
     * half-written records, once older than [ORPHAN_MARGIN_MS]. A copy whose record cannot be read is kept (it may
     * still be the person's only photo). Answers how many files went.
     */
    fun sweepOrphans(now: Long = System.currentTimeMillis()): Int {
        val files = dir.listFiles() ?: return 0
        val records = files.filter { it.name.endsWith(".json") }.map { it.name.removeSuffix(".json") }.toSet()
        var gone = 0
        for (f in files) {
            if (!f.isFile || now - f.lastModified() <= ORPHAN_MARGIN_MS) continue
            val orphan = (f.name.endsWith(".bin") && f.name.removeSuffix(".bin") !in records) || f.name.endsWith(".tmp")
            if (orphan && f.delete()) gone++
        }
        return gone
    }
}

/**
 * A file taken for a record that Hoje tells about: kept on the phone because the core did not take it ([canRetry]:
 * "Tentar de novo" and "Descartar"), or refused by the core for what it is when a start delivered it ("Entendi").
 */
internal data class PendingNotice(val operationId: String, val name: String, val message: String, val canRetry: Boolean)

/** What [PendingAttachFlow] needs of the core (the app's is `Api`; the tests give a fake). Blocking: the worker calls it. */
internal interface PendingCore {
    /** `attachFile` for [p]: returns when attached or queued, throws [CoreException] (its code, its card) or anything else. */
    fun attach(p: PendingAttach)

    /** The core's whole `operationStatus` answer about [operationId]; null when it gave none. */
    fun status(operationId: String): JSONObject?
}

/** Where [PendingAttachFlow]'s work runs (the core's worker) and where its answers land (the main thread). */
internal interface FlowRunner {
    fun <T> run(work: () -> T, done: (Result<T>) -> Unit)
}

/** What became of a file handed to the core ([atResume]: by a start, with nobody waiting on the screen). */
internal sealed class FlowEvent {
    abstract val p: PendingAttach
    abstract val atResume: Boolean

    data class Attached(override val p: PendingAttach, override val atResume: Boolean) : FlowEvent()
    data class KeptInCard(
        override val p: PendingAttach, val cardId: Long, val message: String, override val atResume: Boolean
    ) : FlowEvent()
    data class FileRefused(override val p: PendingAttach, val message: String, override val atResume: Boolean) : FlowEvent()
    data class Kept(override val p: PendingAttach, val message: String, override val atResume: Boolean) : FlowEvent()
}

/**
 * The orchestration of the photos and files taken for a record (F3.1b2; `revisao-f3-1b2.md` findings 1 and 7), apart
 * from Android so it is tested with a fake core: [deliver] (the ficha's "Anexar…"), [resume] (a start, and after the
 * unlock), [retry] and [discard] (Hoje). The staged copy goes only when the core durably took the file, durably kept
 * it in a card, or refused the file itself ([PendingAttachRules.afterAttach]). Everything else keeps it and becomes a
 * [PendingNotice]; nothing is swallowed. One operation is with the core at a time ([delivering]), and every delivery of
 * an earlier run asks the core first whether its operation was committed, so a process death between any two steps
 * never attaches twice.
 *
 * Main thread: the calls, [onEvent] and [onNotices]. The worker: the core and the folder.
 */
internal class PendingAttachFlow(
    private val store: PendingAttachStore,
    private val core: PendingCore,
    private val runner: FlowRunner,
    private val onEvent: (FlowEvent) -> Unit,
    private val onNotices: (List<PendingNotice>) -> Unit
) {
    /** The operations with the core now, or being handed to it, or being discarded (never two at once). */
    private val delivering = HashSet<String>()

    private class Held(val p: PendingAttach, val notice: PendingNotice)

    /** What Hoje shows, by operation. */
    private val held = LinkedHashMap<String, Held>()

    val notices: List<PendingNotice> get() = held.values.map { it.notice }

    /** [p] was just written to disk for the person on the screen: to the core now. */
    fun deliver(p: PendingAttach) = send(p, atResume = false, ask = false)

    /**
     * Every record left by an earlier run (or by this one, not delivered yet) goes to the core, which is asked first
     * whether its operation was committed; the orphan copies are swept before and after.
     */
    fun resume() {
        runner.run({ store.sweepOrphans(); store.all() }) { r ->
            val left = r.getOrElse { return@run }
            held.keys.retainAll(left.map { it.operationId }.toSet() + delivering)
            for (p in left) {
                if (p.refusal.isNotEmpty()) {
                    held[p.operationId] = Held(p, PendingNotice(p.operationId, p.name, p.refusal, canRetry = false))
                } else {
                    send(p, atResume = true, ask = true)
                }
            }
            publish()
            runner.run({ store.sweepOrphans() }) { }
        }
    }

    /** "Tentar de novo" on a file Hoje lists as kept. */
    fun retry(operationId: String) {
        val h = held[operationId] ?: return
        if (h.notice.canRetry) send(h.p, atResume = false, ask = true)
    }

    /** "Descartar" (or "Entendi"): the file and its record go, by the person's choice; never while with the core. */
    fun discard(operationId: String) {
        val h = held[operationId] ?: return
        if (!delivering.add(operationId)) return
        runner.run({ store.remove(h.p) }) { r ->
            delivering.remove(operationId)
            if (r.isSuccess) {
                held.remove(operationId)
                publish()
            }
        }
    }

    private fun send(p: PendingAttach, atResume: Boolean, ask: Boolean) {
        if (!delivering.add(p.operationId)) return
        runner.run({ settle(p, atResume, ask) }) { r ->
            delivering.remove(p.operationId)
            val e = r.getOrElse { FlowEvent.Kept(p, PendingAttachRules.keptMessage(""), atResume) }
            when {
                e is FlowEvent.Kept -> held[p.operationId] = Held(p, PendingNotice(p.operationId, p.name, e.message, true))
                e is FlowEvent.FileRefused && atResume -> held[p.operationId] =
                    Held(p.copy(refusal = e.message), PendingNotice(p.operationId, p.name, e.message, false))
                else -> held.remove(p.operationId)
            }
            onEvent(e)
            publish()
        }
    }

    /** (The worker.) What the core says of [p], and the folder made to match it. */
    private fun settle(p: PendingAttach, atResume: Boolean, ask: Boolean): FlowEvent {
        if (ask) {
            val st = core.status(p.operationId)
            when (PendingAttachRules.next(st)) {
                PendingAttachRules.Next.DELIVER -> Unit
                PendingAttachRules.Next.CLEAN -> {
                    store.remove(p)
                    return FlowEvent.Attached(p, atResume)
                }
                PendingAttachRules.Next.RECOVER -> {
                    store.remove(p)
                    return FlowEvent.KeptInCard(p, st!!.optLong("card_id"), st.s("message"), atResume)
                }
                else -> return FlowEvent.Kept(p, PendingAttachRules.keptMessage(st?.s("error").orEmpty()), atResume)
            }
        }
        val error = try {
            core.attach(p)
            null
        } catch (e: Throwable) {
            e
        }
        return when (PendingAttachRules.afterAttach(error)) {
            PendingAttachRules.Next.CLEAN -> {
                store.remove(p)
                FlowEvent.Attached(p, atResume)
            }
            PendingAttachRules.Next.RECOVER -> {
                store.remove(p)
                FlowEvent.KeptInCard(p, (error as CoreException).cardId, error.message.orEmpty(), atResume)
            }
            PendingAttachRules.Next.REFUSED -> {
                val m = error?.message.orEmpty()
                if (atResume) store.markRefused(p, m) else store.remove(p)
                FlowEvent.FileRefused(p, m, atResume)
            }
            else -> FlowEvent.Kept(p, PendingAttachRules.keptMessage((error as? PonteException)?.message.orEmpty()), atResume)
        }
    }

    private fun publish() = onNotices(notices)
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
