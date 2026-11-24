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
        assertEquals(4, n.size)
    }

    @Test fun theStagingGoesOnlyAfterTheCoreAnsweredForIt() {
        assertEquals(PendingAttachRules.Next.CLEAN, PendingAttachRules.afterAttach(null))
        assertEquals(PendingAttachRules.Next.RECOVER, PendingAttachRules.afterAttach(CoreException("O registro foi apagado em outro aparelho.", "target_gone")))
        // The core refused the file itself (too big, a program…): it said so, and nothing was kept.
        assertEquals(PendingAttachRules.Next.CLEAN, PendingAttachRules.afterAttach(CoreException("Arquivos .exe não são aceitos.")))
        // No answer (the process, the core): the staging stays for the next start.
        assertEquals(PendingAttachRules.Next.KEEP, PendingAttachRules.afterAttach(IllegalStateException("falhou")))
        assertEquals(PendingAttachRules.Next.KEEP, PendingAttachRules.afterAttach(OutOfMemoryError()))
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
