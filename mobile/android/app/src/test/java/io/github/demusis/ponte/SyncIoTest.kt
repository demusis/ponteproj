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
}
