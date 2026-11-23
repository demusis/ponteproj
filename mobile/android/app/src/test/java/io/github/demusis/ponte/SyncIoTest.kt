package io.github.demusis.ponte

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.security.MessageDigest

/** The byte plumbing of the sync: hashes, the size limit, the "changed meanwhile" decision, cleanup. */
class SyncIoTest {
    private fun hex(b: ByteArray) =
        MessageDigest.getInstance("SHA-256").digest(b).joinToString("") { "%02x".format(it) }

    private fun bytes(n: Int) = ByteArray(n) { (it * 31 + 7).toByte() }

    @Test fun copyAndHashEqualsMessageDigestAcrossBufferBoundaries() {
        for (n in listOf(0, 1, 65535, 65536, 65537, 200_000)) {
            val data = bytes(n)
            val out = ByteArrayOutputStream()
            val d = SyncIo.copyHash(ByteArrayInputStream(data), out, SyncIo.MAX_BYTES)
            assertEquals(hex(data), d.sha256)
            assertEquals(n.toLong(), d.length)
            assertArrayEquals(data, out.toByteArray())
        }
    }

    @Test fun hashOnlyDoesNotNeedAnOutput() {
        val data = bytes(100_000)
        assertEquals(hex(data), SyncIo.copyHash(ByteArrayInputStream(data), null, 100_000).sha256)
    }

    @Test fun limitAcceptsTheBoundaryAndStopsOneByteAfter() {
        val ok = SyncIo.copyHash(ByteArrayInputStream(bytes(1000)), null, 1000)
        assertEquals(1000L, ok.length)
        val out = ByteArrayOutputStream()
        try {
            SyncIo.copyHash(ByteArrayInputStream(bytes(1001)), out, 1000)
            fail("should stop past the limit")
        } catch (e: CoreException) {
            assertEquals(SyncIo.TOO_BIG, e.message)
        }
        assertTrue("nothing past the limit is written", out.size() <= 1000)
    }

    @Test fun limitStopsWithoutReadingTheWholeStream() {
        var read = 0L
        val endless = object : java.io.InputStream() {
            override fun read() = 1
            override fun read(b: ByteArray, off: Int, len: Int): Int { read += len; return len }
        }
        try {
            SyncIo.copyHash(endless, null, 300_000)
            fail("should stop")
        } catch (_: CoreException) {
        }
        assertTrue(read <= 300_000 + 64 * 1024)
    }

    @Test fun hashFileMatchesDigest() {
        val f = File.createTempFile("syncio", ".bin")
        try {
            val data = bytes(150_000)
            f.writeBytes(data)
            val d = SyncIo.hashFile(f)
            assertEquals(hex(data), d.sha256)
            assertEquals(150_000L, d.length)
        } finally { f.delete() }
    }

    @Test fun changedMeanwhileNeedsEqualHashAndLength() {
        val a = SyncIo.Digest(hex(bytes(10)), 10)
        assertTrue(SyncIo.unchanged(a, SyncIo.Digest(hex(bytes(10)), 10)))
        assertFalse(SyncIo.unchanged(a, SyncIo.Digest(hex(bytes(11)), 11)))
        // same size, different content
        val other = bytes(10).also { it[3] = (it[3] + 1).toByte() }
        assertFalse(SyncIo.unchanged(a, SyncIo.Digest(hex(other), 10)))
        // same hash text but a different length is still a change
        assertFalse(SyncIo.unchanged(a, SyncIo.Digest(a.sha256, 11)))
    }

    @Test fun cleanupRunsOnSuccessAndOnException() {
        val ok = File.createTempFile("syncio", ".a")
        assertEquals(5, SyncIo.withCleanup(listOf(ok)) { 5 })
        assertFalse(ok.exists())

        val bad = File.createTempFile("syncio", ".b")
        try {
            SyncIo.withCleanup(listOf(bad)) { throw IOException("boom") }
            fail("should propagate")
        } catch (e: IOException) {
            assertEquals("boom", e.message)
        }
        assertFalse(bad.exists())
    }

    @Test fun cleanupAlsoCoversErrorsThatAreNotExceptions() {
        val f = File.createTempFile("syncio", ".c")
        try {
            SyncIo.withCleanup(listOf(f)) { throw OutOfMemoryError("heap") }
            fail("should propagate")
        } catch (_: OutOfMemoryError) {
        }
        assertFalse(f.exists())
    }

    @Test fun aCopyTheCoreKeptAsideIsNeverSweptEvenWhenNamedAfterAWorkingFile() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val archive = File(dir, SyncIo.WORK_PREFIX + "1-divergente-0a1b2c3d4e5f6071" + SyncIo.WORK_SUFFIX).also { it.writeText("x") }
            val stale = File(dir, SyncIo.WORK_PREFIX + "2" + SyncIo.WORK_SUFFIX).also { it.writeText("x") }
            SyncIo.removeStale(dir)
            assertTrue("o arquivo guardado ao lado é o único exemplar do trabalho", archive.exists())
            assertFalse(stale.exists())
        } finally { dir.deleteRecursively() }
    }

    @Test fun thePendingCopyGoesOnlyOnTheCoresExplicitWord() {
        assertTrue(SyncIo.mayDropPending(true))
        // An answer that does not say it (or says no) keeps the copy.
        assertFalse(SyncIo.mayDropPending(false))
    }

    @Test fun staleWorkFilesGoButOtherFilesStay() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val stale = File(dir, SyncIo.WORK_PREFIX + "1" + SyncIo.WORK_SUFFIX).also { it.writeText("x") }
            val pending = File(dir, "pendente.ponte").also { it.writeText("x") }
            SyncIo.removeStale(dir)
            assertFalse(stale.exists())
            assertTrue(pending.exists())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun theCoresLeftoverTemporaryFilesAreSweptToo() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val leftover = File(dir, "." + SyncIo.WORK_PREFIX + "1" + SyncIo.WORK_SUFFIX + ".tmp-abc").also { it.writeText("x") }
            val other = File(dir, ".outro").also { it.writeText("x") }
            SyncIo.removeStale(dir)
            assertFalse(leftover.exists())
            assertTrue(other.exists())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun theNewOutputNeverReplacesAPendingCopyTheCoreDoesNotCallSettled() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val work = File(dir, "sync-work-1.ponte").also { it.writeBytes(bytes(1_000)) }
            val pending = File(dir, "pendente.ponte").also { it.writeText("trabalho só aqui") }
            // Unsettled (or an answer without the word): the pending copy is untouched.
            val kept = SyncIo.keepNewOutput(work, pending, false)
            assertEquals("pendente-novo.ponte", kept.name)
            assertEquals("trabalho só aqui", pending.readText())
            assertArrayEquals(bytes(1_000), kept.readBytes())
            // Settled: the output becomes the pending copy, as before.
            val again = SyncIo.keepNewOutput(work, pending, true)
            assertEquals(pending, again)
            assertArrayEquals(bytes(1_000), pending.readBytes())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun aStagedCopyStillWaitingIsNeverReplacedByTheNextOutput() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val work = File(dir, "sync-work-1.ponte").also { it.writeBytes(bytes(1_000)) }
            val pending = File(dir, "pendente.ponte").also { it.writeText("trabalho só aqui") }
            val staged = File(dir, "pendente-novo.ponte").also { it.writeText("saída anterior") }
            val kept = SyncIo.keepNewOutput(work, pending, false)
            assertEquals("pendente-novo-2.ponte", kept.name)
            assertEquals("saída anterior", staged.readText())
            assertEquals("trabalho só aqui", pending.readText())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun eachPendingCopyIsSettledOnItsOwn() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val a = File(dir, "pendente.ponte").also { it.writeText("a") }
            val b = File(dir, "pendente-novo.ponte").also { it.writeText("b") }
            val work = File(dir, "work.ponte").absolutePath
            // One blocked copy does not pin the other.
            assertEquals(listOf(b), SyncIo.settledPendings(listOf(a, b), work, listOf(a.absolutePath), false))
            // The working copy unsettled: nothing reached Drive, none goes.
            assertEquals(emptyList<File>(), SyncIo.settledPendings(listOf(a, b), work, listOf(work), true))
            // An older core: all or none.
            assertEquals(listOf(a, b), SyncIo.settledPendings(listOf(a, b), work, null, true))
            assertEquals(emptyList<File>(), SyncIo.settledPendings(listOf(a, b), work, null, false))
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun anOutputAlreadyStagedIsReusedNotStagedAgain() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val work = File(dir, "sync-work-1.ponte").also { it.writeBytes(bytes(1_000)) }
            val pending = File(dir, "pendente.ponte").also { it.writeText("trabalho só aqui") }
            val first = SyncIo.keepNewOutput(work, pending, false)
            val again = SyncIo.keepNewOutput(work, pending, false)
            assertEquals(first, again)
            assertEquals(listOf("pendente-novo.ponte", "pendente.ponte", "sync-work-1.ponte"), dir.list()!!.sorted())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun everyPendingCopyIsReadAgainButNotTheArchivesNorTheParts() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            for (n in listOf(
                "pendente.ponte", "pendente-novo.ponte", "pendente-novo-2.ponte",
                "pendente-divergente-0a1b2c3d4e5f6071.ponte", "pendente-novo.ponte.parte", "acervo.sqlite3",
            )) File(dir, n).writeText("x")
            assertEquals(
                listOf("pendente-novo-2.ponte", "pendente-novo.ponte", "pendente.ponte"),
                SyncIo.pendingCopies(dir).map { it.name },
            )
            assertTrue(SyncIo.pendingCopies(File(dir, "ausente")).isEmpty())
        } finally { dir.deleteRecursively() }
    }

    @Test
    fun keepingThePendingCopyReplacesItWholeAndLeavesNoPart() {
        val dir = java.nio.file.Files.createTempDirectory("syncio").toFile()
        try {
            val from = File(dir, "novo").also { it.writeBytes(bytes(200_000)) }
            val to = File(dir, "pendente.ponte").also { it.writeText("antigo") }
            SyncIo.keep(from, to)
            assertArrayEquals(bytes(200_000), to.readBytes())
            assertTrue(from.exists())
            assertEquals(listOf("novo", "pendente.ponte"), dir.list()!!.sorted())
            // A source that cannot be read leaves the old copy as it was.
            try { SyncIo.keep(File(dir, "ausente"), to); fail("deveria falhar") } catch (_: IOException) { }
            assertArrayEquals(bytes(200_000), to.readBytes())
            assertEquals(listOf("novo", "pendente.ponte"), dir.list()!!.sorted())
        } finally { dir.deleteRecursively() }
    }

    /**
     * F2b (B15, review F2B-05): the upload is written once, only while the Drive file is still the
     * one read; a stub that arrives during the upload is never overwritten, and a changed file is
     * never written at all.
     */
    @Test fun aStubArrivingDuringTheUploadIsNeverOverwritten() {
        val original = SyncIo.Digest(hex(bytes(10)), 10)
        val want = SyncIo.Digest(hex(bytes(12)), 12)
        val stub = SyncIo.Digest(hex(bytes(14)), 14)
        // The write lands, then the cloud delivers the stub before the read-back.
        var drive = original
        var writes = 0
        val ended = SyncIo.uploadOnce(original, want, readNow = { drive }, write = { writes++; drive = stub })
        assertEquals(SyncIo.Upload.NOT_CONFIRMED, ended)
        assertEquals(1, writes)
        assertEquals(stub, drive)
        // The Drive file is not the one read: nothing is written.
        drive = stub
        writes = 0
        assertEquals(SyncIo.Upload.CHANGED, SyncIo.uploadOnce(original, want, readNow = { drive }, write = { writes++ }))
        assertEquals(0, writes)
        // The usual case.
        drive = original
        assertEquals(SyncIo.Upload.DONE, SyncIo.uploadOnce(original, want, readNow = { drive }, write = { drive = want }))
    }
}
