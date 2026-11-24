package io.github.demusis.ponte

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.UUID

/**
 * `revisao-f3-1b2.md` findings 1 and 7: the orchestration of a photo or file taken for a record ([PendingAttachFlow]:
 * what `Intake.deliver`, `runAttach` and `resumePending` do), against a fake core. The staged copy goes only when the
 * core durably took the file (attached or queued), durably kept it in a card, or refused the file itself; any other
 * answer keeps it, and the person is told (on the screen at attach time, in Hoje for a file a start could not
 * deliver), with "Tentar de novo" and "Descartar". A process death between any two steps resumes the same operation and
 * never attaches twice.
 */
class PendingAttachFlowTest {
    private lateinit var dir: File
    private lateinit var folder: File
    private lateinit var store: PendingAttachStore

    @Before fun setUp() {
        dir = Files.createTempDirectory("fluxo-anexo").toFile()
        folder = File(dir, PendingAttachStore.DIR)
        store = PendingAttachStore(folder)
    }

    @After fun tearDown() {
        dir.deleteRecursively()
    }

    /**
     * The core as the app sees it: an operation it committed answers the same forever (attached, queued, or refused
     * into a card), and a repeat of it commits nothing more ([commits] counts what each operation made).
     */
    private class FakeCore : PendingCore {
        val committed = HashMap<String, JSONObject>()
        val commits = HashMap<String, Int>()
        /** What the next calls of [attach] do, in order; with none left, the file is attached. */
        val script = ArrayDeque<(PendingAttach) -> Unit>()
        var statusAnswers = true

        fun ok(p: PendingAttach) {
            committed[p.operationId] = JSONObject().put("kind", "attach").put("outcome", "pending").put("document_id", p.docId)
            commits.merge(p.operationId, 1, Int::plus)
        }

        override fun attach(p: PendingAttach) {
            committed[p.operationId]?.let { done ->
                if (done.optString("outcome") == "refused") {
                    throw CoreException(done.optString("message"), "target_gone", done.optLong("card_id"))
                }
                return
            }
            (script.removeFirstOrNull() ?: ::ok)(p)
        }

        override fun status(operationId: String): JSONObject? {
            if (!statusAnswers) return null
            val done = committed[operationId] ?: return JSONObject("""{"ok":true,"done":false}""")
            return JSONObject(done.toString()).put("ok", true).put("done", true)
        }

        fun keepsInCard(card: Long, message: String): (PendingAttach) -> Unit = { p ->
            committed[p.operationId] = JSONObject().put("kind", "attach").put("outcome", "refused")
                .put("card_id", card).put("message", message)
            commits.merge(p.operationId, 1, Int::plus)
            throw CoreException("$message O arquivo não foi anexado: ficou guardado neste aparelho.", "target_gone", card)
        }

        fun fails(e: Throwable): (PendingAttach) -> Unit = { throw e }

        /** The core commits and the process dies before the app hears of it. */
        fun commitsAndDies(): (PendingAttach) -> Unit = { p ->
            ok(p)
            throw IllegalStateException("o processo morreu")
        }
    }

    /** Runs each work at once, as if the worker and the main thread were one. */
    private object AtOnce : FlowRunner {
        override fun <T> run(work: () -> T, done: (Result<T>) -> Unit) = done(runCatching(work))
    }

    /** Holds the works until [step]: a process death is dropping what is left. */
    private class Held : FlowRunner {
        val queue = ArrayDeque<() -> Unit>()
        override fun <T> run(work: () -> T, done: (Result<T>) -> Unit) {
            queue.addLast { done(runCatching(work)) }
        }
        fun step() = queue.removeFirst().invoke()
        fun drain() { while (queue.isNotEmpty()) step() }
    }

    private val events = mutableListOf<FlowEvent>()
    private var notices = emptyList<PendingNotice>()

    private fun flow(core: PendingCore, runner: FlowRunner = AtOnce) =
        PendingAttachFlow(store, core, runner, { events += it }, { notices = it })

    private fun photo(name: String = "foto.jpg", bytes: String = "ÿØ foto") =
        File(dir, "trabalho-" + UUID.randomUUID() + ".tmp").apply { writeText(bytes) }
            .let { store.adopt(it, 7L, AttachProof("4f1c0a", "00000000000000a1"), name, "Foto tirada no celular") }

    private fun assertKept(p: PendingAttach) {
        assertEquals(listOf(p), store.all())
        assertTrue("the staged copy stays", File(p.staged).isFile)
    }

    private fun assertGone(p: PendingAttach) {
        assertTrue(store.all().none { it.operationId == p.operationId })
        assertFalse(File(p.staged).exists())
    }

    @Test fun attachedCleansAndSaysSo() {
        val core = FakeCore()
        val p = photo()
        flow(core).deliver(p)
        assertGone(p)
        assertEquals(1, core.commits[p.operationId])
        assertEquals(listOf<FlowEvent>(FlowEvent.Attached(p, atResume = false)), events)
        assertTrue(notices.isEmpty())
    }

    @Test fun keptInACardCleansAndLeadsToTheCard() {
        val core = FakeCore()
        core.script += core.keepsInCard(4L, "O registro foi apagado em outro aparelho.")
        val p = photo()
        flow(core).deliver(p)
        assertGone(p)
        val e = events.single() as FlowEvent.KeptInCard
        assertEquals(4L, e.cardId)
        assertFalse(e.atResume)
        assertTrue(e.message, e.message.startsWith("O registro foi apagado em outro aparelho."))
        assertTrue(notices.isEmpty())
    }

    @Test fun anyOtherErrorKeepsTheFileAndTellsWithARetry() {
        val errors = listOf(
            CoreException("Falha interna no núcleo do Ponte."),
            CoreException("O acervo ficou indisponível; feche e abra o aplicativo."),
            CoreException("database or disk is full"),
            CoreException("Arquivos .exe não são aceitos."),
            CoreException("O documento escolhido mudou.", "target_changed"),
            CoreException("Sem cartão.", "target_gone"),
            IllegalStateException("falhou"),
            OutOfMemoryError()
        )
        for (error in errors) {
            events.clear()
            val core = FakeCore()
            core.script += core.fails(error)
            val p = photo()
            val f = flow(core)
            f.deliver(p)
            assertKept(p)
            val e = events.single()
            assertTrue("$error: $e", e is FlowEvent.Kept && !e.atResume)
            val n = notices.single()
            assertEquals(PendingNotice(p.operationId, p.name, n.message, canRetry = true), n)
            assertTrue(n.message, n.message.contains("continua guardado"))
            if (error is CoreException) assertTrue(n.message, n.message.contains(error.message!!.removeSuffix(".")))
            // "Tentar de novo": the core takes it now; the notice goes.
            f.retry(p.operationId)
            assertGone(p)
            assertTrue(events.last() is FlowEvent.Attached)
            assertTrue(notices.isEmpty())
            assertEquals(1, core.commits[p.operationId])
        }
    }

    @Test fun aRefusalOfTheFileItselfCleansAndIsSaidOnTheScreen() {
        val core = FakeCore()
        core.script += core.fails(CoreException("Arquivos .exe não são aceitos.", "file_refused"))
        val p = photo("programa.exe")
        flow(core).deliver(p)
        assertGone(p)
        assertEquals(listOf<FlowEvent>(FlowEvent.FileRefused(p, "Arquivos .exe não são aceitos.", atResume = false)), events)
        assertTrue(notices.isEmpty())
    }

    @Test fun aStartNeverSwallowsAnError() {
        // The core failed for a file left by an earlier run: kept, and Hoje says so.
        val core = FakeCore()
        core.script += core.fails(CoreException("Falha interna no núcleo do Ponte."))
        val p = photo()
        val f = flow(core)
        f.resume()
        assertKept(p)
        assertTrue(events.single() is FlowEvent.Kept)
        assertEquals(listOf(p.operationId), notices.map { it.operationId })
        assertTrue(notices.single().canRetry)

        // The core does not answer about the operation at all: kept and told too.
        events.clear()
        core.statusAnswers = false
        f.resume()
        assertKept(p)
        assertEquals(listOf(p.operationId), notices.map { it.operationId })
        assertEquals(0, core.commits[p.operationId] ?: 0)

        // The core is back: the next start delivers it.
        core.statusAnswers = true
        f.resume()
        assertGone(p)
        assertTrue(notices.isEmpty())
        assertEquals(1, core.commits[p.operationId])
    }

    @Test fun aFileRefusedAtStartIsToldInHojeAcrossAProcessDeath() {
        val core = FakeCore()
        core.script += core.fails(CoreException("O conteúdo de “foto.jpg” não corresponde a um arquivo .jpg.", "file_refused"))
        val p = photo()
        flow(core).resume()
        assertFalse("the bytes no record takes go", File(p.staged).exists())
        val e = events.single() as FlowEvent.FileRefused
        assertTrue(e.atResume)
        val n = notices.single()
        assertEquals(p.operationId, n.operationId)
        assertFalse("nothing to try again", n.canRetry)
        assertTrue(n.message, n.message.contains("não corresponde"))

        // The process dies before the person saw it: the next start still tells, and never sends it again.
        notices = emptyList()
        val next = flow(core)
        next.resume()
        assertEquals(listOf(n), notices)
        assertEquals(0, core.commits[p.operationId] ?: 0)
        // "Entendi" (Descartar): gone for good.
        next.discard(p.operationId)
        assertTrue(notices.isEmpty())
        assertTrue(store.all().isEmpty())
        assertTrue(folder.listFiles()!!.none { it.name.startsWith(p.operationId) })
    }

    @Test fun aStartFindsTheCardTheCoreKept() {
        val core = FakeCore()
        val p = photo()
        core.committed[p.operationId] = JSONObject().put("kind", "attach").put("outcome", "refused")
            .put("card_id", 9L).put("message", "O registro está na Lixeira.")
        flow(core).resume()
        assertGone(p)
        assertEquals(listOf<FlowEvent>(FlowEvent.KeptInCard(p, 9L, "O registro está na Lixeira.", atResume = true)), events)
        assertTrue(notices.isEmpty())
    }

    @Test fun aProcessDeathBeforeTheCoreResumesTheSameOperationOnce() {
        val core = FakeCore()
        val p = photo()
        val held = Held()
        flow(core, held).deliver(p)
        // The process dies before the worker ran: the work is lost, the file and its record are not.
        held.queue.clear()
        assertKept(p)
        val next = flow(core)
        next.resume()
        assertGone(p)
        assertEquals(mapOf(p.operationId to 1), core.commits)
        assertEquals(p.operationId, core.committed.keys.single())
    }

    @Test fun aProcessDeathAfterTheCoreNeverAttachesTwice() {
        val core = FakeCore()
        core.script += core.commitsAndDies()
        val p = photo()
        flow(core).deliver(p)
        // The core committed; the app never heard: the file stays (nothing is guessed).
        assertKept(p)
        val next = flow(core)
        next.resume()
        assertGone(p)
        assertEquals(mapOf(p.operationId to 1), core.commits)
        assertTrue(notices.isEmpty())
    }

    @Test fun twoStartsAtOnceDeliverOnce() {
        val core = FakeCore()
        val p = photo()
        val held = Held()
        val f = flow(core, held)
        f.resume()
        f.resume()
        held.drain()
        f.resume()
        held.drain()
        assertGone(p)
        assertEquals(mapOf(p.operationId to 1), core.commits)
        assertEquals(1, events.count { it is FlowEvent.Attached })
    }

    @Test fun descartarLetsTheFileGoButNeverWhileTheCoreHasIt() {
        val core = FakeCore()
        core.script += core.fails(CoreException("Falha interna no núcleo do Ponte."))
        val p = photo()
        val held = Held()
        val f = flow(core, held)
        f.deliver(p)
        held.drain()
        assertEquals(1, notices.size)
        // A retry is with the core: "Descartar" waits.
        f.retry(p.operationId)
        f.discard(p.operationId)
        held.drain()
        assertGone(p)
        assertEquals(1, core.commits[p.operationId])
        assertTrue(notices.isEmpty())

        val q = photo("outra.jpg")
        core.script += core.fails(CoreException("Falha interna no núcleo do Ponte."))
        f.deliver(q)
        held.drain()
        f.discard(q.operationId)
        held.drain()
        assertGone(q)
        assertTrue(notices.isEmpty())
        assertEquals(null, core.commits[q.operationId])
    }
}
