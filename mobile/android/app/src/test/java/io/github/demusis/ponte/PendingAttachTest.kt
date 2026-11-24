package io.github.demusis.ponte

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.UUID

/**
 * F3.1b2 (`desenho-f3-v3-adendo.md` §10, frozen rules 19-22), the app's half of
 * `f3_1b2_phone_op_survives_refusals_and_space_change` (the core's half is in `mobile/rust`): a photo or file taken
 * for a record is on disk, with the record's identity and the library space, before the core sees it; it survives the
 * process; its staging goes only once the core answered for it; a refusal opens the recovery screen, whose three
 * actions share one operation per card, persisted before the call.
 */
class PendingAttachTest {
    private lateinit var dir: File

    @Before fun setUp() {
        dir = Files.createTempDirectory("anexo-pendente").toFile()
    }

    @After fun tearDown() {
        dir.deleteRecursively()
    }

    private fun work(bytes: String = "%PDF-1.4 foto") = File(dir, "trabalho-" + UUID.randomUUID() + ".tmp").apply { writeText(bytes) }

    private val proof = AttachProof(ident = "4f1c0a", space = "00000000000000a1")

    @Test fun aPendingAttachIsOnDiskBeforeTheCoreAndSurvivesTheProcess() {
        val folder = File(dir, PendingAttachStore.DIR)
        val store = PendingAttachStore(folder)
        val w = work()
        val p = store.adopt(w, docId = 7L, proof = proof, name = "foto.jpg", description = "Foto tirada no celular")
        assertFalse("the working file became the staging", w.exists())
        val staged = File(p.staged)
        assertTrue(staged.isFile)
        assertEquals(folder.canonicalPath, staged.parentFile!!.canonicalPath)
        assertEquals("%PDF-1.4 foto", staged.readText())
        assertEquals(7L, p.docId)
        assertEquals(proof.ident, p.docIdent)
        assertEquals(proof.space, p.librarySpace)
        UUID.fromString(p.operationId)
        // The process dies: another run reads the same record back.
        assertEquals(listOf(p), PendingAttachStore(folder).all())
        // A damaged record is skipped, never fatal; the good one stays.
        File(folder, "lixo.json").writeText("{ isto não é json")
        assertEquals(listOf(p), PendingAttachStore(folder).all())
    }

    @Test fun theStagingAndTheRecordGoOnlyTogether() {
        val folder = File(dir, PendingAttachStore.DIR)
        val store = PendingAttachStore(folder)
        val a = store.adopt(work("a"), 7L, proof, "a.pdf", "")
        val b = store.adopt(work("b"), 8L, proof, "b.pdf", "")
        store.remove(a)
        assertFalse(File(a.staged).exists())
        assertEquals(listOf(b), store.all())
        // A record never deletes a file outside its folder.
        val outside = work("fora")
        store.remove(b.copy(staged = outside.absolutePath))
        assertTrue(outside.exists())
    }

    @Test fun theOptionsCarryTheOperationAndTheProofTheAppHeld() {
        val p = PendingAttachStore(File(dir, PendingAttachStore.DIR)).adopt(work(), 7L, proof, "foto.jpg", "")
        val o = JSONObject(PendingAttachRules.options(p))
        assertEquals(p.operationId, o.getString("operation_id"))
        assertEquals(proof.ident, o.getString("document_ident"))
        assertEquals(proof.space, o.getString("library_space"))
    }

    private fun status(json: String?) = PendingAttachRules.next(json?.let { JSONObject(it) })

    @Test fun whatAPendingAttachFoundAtStartBecomesIsWhatTheCoreSays() {
        val n = PendingAttachRules.Next.values()
        // Not committed: delivered again, with the same operation (never twice).
        assertEquals(PendingAttachRules.Next.DELIVER, status("""{"ok":true,"done":false}"""))
        // Committed: attached or queued, or refused into a card the recovery screen lists.
        assertEquals(PendingAttachRules.Next.CLEAN, status("""{"ok":true,"done":true,"kind":"attach","outcome":"pending"}"""))
        assertEquals(PendingAttachRules.Next.CLEAN, status("""{"ok":true,"done":true,"kind":"attach","outcome":"accepted"}"""))
        assertEquals(PendingAttachRules.Next.RECOVER, status("""{"ok":true,"done":true,"kind":"attach","outcome":"refused","card_id":4}"""))
        // No answer, an error or another kind of operation: kept, never guessed.
        assertEquals(PendingAttachRules.Next.KEEP, status(null))
        assertEquals(PendingAttachRules.Next.KEEP, status("""{"ok":false,"error":"x"}"""))
        assertEquals(PendingAttachRules.Next.KEEP, status("""{"ok":true}"""))
        assertEquals(PendingAttachRules.Next.KEEP, status("""{"ok":true,"done":true,"kind":"record"}"""))
        // Refused with no card to recover it from: nothing proves where the file is, so it stays.
        assertEquals(PendingAttachRules.Next.KEEP, status("""{"ok":true,"done":true,"kind":"attach","outcome":"refused"}"""))
        assertEquals(5, n.size)
    }

    /**
     * `revisao-f3-1b2.md` finding 1 (HIGH): the staging goes only for an answer that says where the file is. Attached;
     * kept in card N (`target_gone` WITH its card); or refused for what the file itself is (`file_refused`), which is
     * said on the screen. Every other error (disk full, a panic, the library lock, an unknown text, a code without its
     * card) keeps it for "Tentar de novo".
     */
    @Test fun theStagingGoesOnlyAfterTheCoreAnsweredForIt() {
        val after = PendingAttachRules::afterAttach
        assertEquals(PendingAttachRules.Next.CLEAN, after(null))
        assertEquals(PendingAttachRules.Next.RECOVER, after(CoreException("O registro foi apagado em outro aparelho.", "target_gone", 4L)))
        assertEquals(PendingAttachRules.Next.KEEP, after(CoreException("O registro foi apagado em outro aparelho.", "target_gone")))
        assertEquals(PendingAttachRules.Next.REFUSED, after(CoreException("Arquivos .exe não são aceitos.", "file_refused")))
        // No code: the core did not say the file is anywhere (its old wording of a refusal included).
        for (m in listOf(
            "Arquivos .exe não são aceitos.", "Falha interna no núcleo do Ponte.",
            "O acervo ficou indisponível; feche e abra o aplicativo.", "database or disk is full"
        )) assertEquals(m, PendingAttachRules.Next.KEEP, after(CoreException(m)))
        assertEquals(PendingAttachRules.Next.KEEP, after(CoreException("O documento escolhido mudou.", "target_changed")))
        // No answer (the process, the core): the staging stays for the next start.
        assertEquals(PendingAttachRules.Next.KEEP, after(IllegalStateException("falhou")))
        assertEquals(PendingAttachRules.Next.KEEP, after(OutOfMemoryError()))
    }

    @Test fun theNoticeOfAKeptFileSaysWhyAndThatItIsStillHere() {
        val m = PendingAttachRules.keptMessage("O acervo ficou indisponível; feche e abra o aplicativo.")
        assertTrue(m, m.contains("O acervo ficou indisponível; feche e abra o aplicativo"))
        assertTrue(m, m.contains("continua guardado"))
        assertTrue(PendingAttachRules.keptMessage("").contains("continua guardado"))
        assertEquals("", PendingAttachRules.noticeBanner(0))
        assertEquals("1 arquivo guardado no celular ainda não foi anexado", PendingAttachRules.noticeBanner(1))
        assertEquals("2 arquivos guardados no celular ainda não foram anexados", PendingAttachRules.noticeBanner(2))
    }

    /** Finding 1: a file the core refused at a start leaves its notice on disk (the bytes go), until "Entendi". */
    @Test fun aRefusalNoticeOutlivesTheProcessWithoutTheBytes() {
        val folder = File(dir, PendingAttachStore.DIR)
        val store = PendingAttachStore(folder)
        val p = store.adopt(work(), 7L, proof, "foto.jpg", "")
        store.markRefused(p, "Arquivos .exe não são aceitos.")
        assertFalse(File(p.staged).exists())
        val back = PendingAttachStore(folder).all().single()
        assertEquals(p.copy(refusal = "Arquivos .exe não são aceitos."), back)
        PendingAttachStore(folder).remove(back)
        assertTrue(PendingAttachStore(folder).all().isEmpty())
    }

    /**
     * Finding 5 (LOW): a staged copy whose record was never written (the process died in between) is swept at a start
     * and after it, once older than a safe margin; a fresh one (being adopted now), one with its record and a refusal
     * notice are kept.
     */
    @Test fun anOrphanCopyIsSweptOnlyOnceItIsSurelyOrphan() {
        val folder = File(dir, PendingAttachStore.DIR)
        val store = PendingAttachStore(folder)
        val kept = store.adopt(work("guardada"), 7L, proof, "a.pdf", "")
        val notice = store.adopt(work("recusada"), 7L, proof, "b.pdf", "")
        store.markRefused(notice, "Recusado.")
        val now = System.currentTimeMillis()
        val old = now - PendingAttachStore.ORPHAN_MARGIN_MS - 1_000
        val orphan = File(folder, UUID.randomUUID().toString() + ".bin").apply { writeText("órfã"); setLastModified(old) }
        val fresh = File(folder, UUID.randomUUID().toString() + ".bin").apply { writeText("chegando") }
        val tmp = File(folder, UUID.randomUUID().toString() + ".json.tmp").apply { writeText("{"); setLastModified(old) }
        File(kept.staged).setLastModified(old)
        assertEquals(2, store.sweepOrphans(now))
        assertFalse(orphan.exists())
        assertFalse(tmp.exists())
        assertTrue(fresh.exists())
        assertTrue(File(kept.staged).exists())
        assertEquals(setOf(kept.operationId, notice.operationId), store.all().map { it.operationId }.toSet())
        // Adopting marks the copy as new, whatever the age of the file it came from.
        val w = work("antiga").apply { setLastModified(old) }
        val adopted = store.adopt(w, 7L, proof, "c.pdf", "")
        assertTrue(File(adopted.staged).lastModified() >= now - 60_000)
    }

    /** Finding 4 (LOW): "Ver em Hoje" leaves the person where they are while anything unsaved is on the stack. */
    @Test fun verEmHojeNeverDropsWhatIsBeingTyped() {
        assertTrue(PendingAttachRules.seeInToday(emptyList(), attachBusy = false))
        assertTrue(PendingAttachRules.seeInToday(listOf(Route.Document(7L)), attachBusy = false))
        assertFalse(PendingAttachRules.seeInToday(listOf(Route.NewTask(7L)), attachBusy = false))
        assertFalse(PendingAttachRules.seeInToday(listOf(Route.Document(7L), Route.Note(7L)), attachBusy = false))
        assertFalse(PendingAttachRules.seeInToday(emptyList(), attachBusy = true))
    }

    @Test fun aRecoveryOperationIsPersistedFirstAndStaysTheSameForItsCard() {
        val file = File(dir, RecoveryOps.FILE)
        val ops = RecoveryOps(file)
        val a = ops.opFor(12L)
        UUID.fromString(a)
        assertTrue("written before it is used", file.isFile)
        // After a process death, the same card gets the same operation: whatever action the person takes then,
        // the core makes it at most once.
        assertEquals(a, RecoveryOps(file).opFor(12L))
        assertNotEquals(a, ops.opFor(13L))
        RecoveryOps(file).forget(12L)
        assertNotEquals(a, RecoveryOps(file).opFor(12L))
        assertEquals(ops.opFor(13L), RecoveryOps(file).opFor(13L))
    }

    @Test fun theRefusedFilesAreReadFromTheCore() {
        val answer = JSONObject(
            """{"ok":true,"items":[{"operation_id":"op-1","card_id":4,"name":"foto.jpg","size":1200,
               "message":"O registro foi apagado em outro aparelho."},{"card_id":0,"name":"sem cartão"}]}"""
        )
        val list = PendingAttachRules.refused(answer)
        assertEquals(listOf(RefusedFile(4L, "foto.jpg", 1200L, "O registro foi apagado em outro aparelho.")), list)
        assertEquals(
            listOf("Criar registro com esta foto", "Anexar a outro registro…", "Descartar"),
            PendingAttachRules.ACTIONS
        )
        assertEquals("1 arquivo do celular não foi anexado", PendingAttachRules.banner(1))
        assertEquals("3 arquivos do celular não foram anexados", PendingAttachRules.banner(3))
        assertEquals("", PendingAttachRules.banner(0))
    }

    @Test fun theFichaGivesTheProofOfItsRecord() {
        val d = JSONObject("""{"id":7,"number":"N","title":"T","proof":{"ident":"4f1c0a","library_space":"00000000000000a1"}}""").toDocDetail()
        assertEquals(proof, d.proof)
        // An older core says nothing: no proof, the attach goes as before.
        assertEquals(AttachProof("", ""), JSONObject("""{"id":7}""").toDocDetail().proof)
    }
}
