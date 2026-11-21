package io.github.demusis.ponte

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * The order of the draft's writes (revisions, one queue, the marker before the core), the one size limit of the
 * file, the age checked before anything is hashed, and the quarantine of a newer app's draft.
 */
class RascunhoOrdemTest {
    private lateinit var dir: File
    private lateinit var store: ShareDraftStore
    private val now = 1_800_000_000_000L
    private val day = 24L * 60 * 60 * 1000
    private val opId = "0a1b2c3d-0000-4000-8000-123456789abc"
    private val exec = Executors.newSingleThreadExecutor()

    @Before fun setUp() {
        dir = Files.createTempDirectory("rascunho-ordem").toFile()
        store = ShareDraftStore(File(dir, "rascunho"))
    }

    @After fun tearDown() {
        exec.shutdownNow()
        dir.deleteRecursively()
    }

    private fun draft(
        title: String = "título", text: String = "corpo", copy: String = "", saving: DraftSaving? = null,
        createdAt: Long = now - 1000
    ) = ShareDraft(
        createdAt = createdAt, updatedAt = now, copy = copy, name = if (copy.isEmpty()) "" else "scan.pdf",
        size = if (copy.isEmpty()) 0 else 10, mime = "", hash = "", text = text, subject = "", mode = "RECORD",
        kind = "outro", title = title, date = "", origin = "", notes = "", targetId = 0, targetNumber = "",
        targetTitle = "", description = "", saving = saving
    )

    private val marker = DraftSaving(ShareDraftRules.OP_RECORD, opId, now)

    private fun onDisk(): ShareDraft = (store.read() as ShareDraftStore.Read.Found).draft

    // ------------------------------------------------------------------ revisions

    @Test fun aLateWriteOfALowerRevisionIsRefusedAndTheMarkerStays() {
        // The marker snapshot (revision 5) is on the disk; the debounce worker's older snapshot (3) arrives late.
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(draft(saving = marker), 5))
        assertEquals(ShareDraftStore.Wrote.STALE, store.writeRevision(draft(title = "antigo"), 3))
        assertEquals(marker, onDisk().saving)
        assertEquals("título", onDisk().title)
        // The same revision again and a higher one are fine.
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(draft(title = "igual", saving = marker), 5))
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(draft(title = "novo"), 6))
        assertEquals(null, onDisk().saving)
        assertEquals("novo", onDisk().title)
        // A refusal touches nothing: no temporary file left behind.
        assertEquals(listOf(ShareDraftStore.FILE), File(dir, "rascunho").list()!!.toList())
    }

    @Test fun aFailedWriteDoesNotMoveTheLastRevision() {
        val blocked = File(dir, "arquivo").apply { writeText("x") }
        val broken = ShareDraftStore(File(blocked, "dentro"))
        assertEquals(ShareDraftStore.Wrote.FAILED, broken.writeRevision(draft(), 9))
        assertEquals(ShareDraftStore.Wrote.FAILED, broken.writeRevision(draft(), 8))
    }

    @Test fun everyWriteGoesThroughOneQueueInTheOrderAsked() {
        val w = DraftWriter(store, exec)
        val gate = java.util.concurrent.CountDownLatch(1)
        // The first job holds the thread: the writes asked meanwhile wait their turn, in order.
        w.submit { gate.await(5, TimeUnit.SECONDS) }
        val fs = (1..20).map { w.write(draft(title = "t$it"), it.toLong()) }
        gate.countDown()
        for (f in fs) assertEquals(ShareDraftStore.Wrote.OK, f.get(5, TimeUnit.SECONDS))
        assertEquals("t20", onDisk().title)
    }

    @Test fun anOlderSnapshotQueuedAfterTheMarkerNeverWins() {
        val w = DraftWriter(store, exec)
        // What the old code did: the marker snapshot (rev 2) is written, then a snapshot of rev 1 (taken before the
        // marker) reaches the writer. The store refuses it.
        val marked = w.write(draft(saving = marker), 2)
        val late = w.write(draft(), 1)
        assertEquals(ShareDraftStore.Wrote.OK, marked.get(5, TimeUnit.SECONDS))
        assertEquals(ShareDraftStore.Wrote.STALE, late.get(5, TimeUnit.SECONDS))
        assertEquals(marker, onDisk().saving)
    }

    @Test fun theMarkerIsOnTheDiskBeforeTheCoreIsCalled() {
        val log = java.util.Collections.synchronizedList(ArrayList<String>())
        val w = DraftWriter(store, exec)
        val f = w.write(draft(saving = marker), 1) { log += "marcador gravado" }
        val r = ShareDraftRules.callAfterMarker({ ShareDraftRules.durable(f) }) {
            // At this moment the file already holds the marker.
            log += "núcleo chamado com marcador=" + (onDisk().saving == marker)
            42
        }
        assertEquals(42, r)
        assertEquals(listOf("marcador gravado", "núcleo chamado com marcador=true"), log)
    }

    @Test fun aMarkerThatCannotBeWrittenKeepsTheCoreFromBeingCalled() {
        val blocked = File(dir, "arquivo").apply { writeText("x") }
        val w = DraftWriter(ShareDraftStore(File(blocked, "dentro")), exec)
        val f = w.write(draft(saving = marker), 1)
        var called = false
        try {
            ShareDraftRules.callAfterMarker({ ShareDraftRules.durable(f) }) { called = true }
            org.junit.Assert.fail("o núcleo não devia ser chamado")
        } catch (e: PonteException) {
            assertEquals(ShareDraftRules.MSG_CANNOT_PREPARE, e.message)
        }
        assertFalse(called)
        // A write that never finishes in time is not "durable" either.
        val never = java.util.concurrent.FutureTask<ShareDraftStore.Wrote> { ShareDraftStore.Wrote.OK }
        assertFalse(ShareDraftRules.durable(never, 20))
    }

    @Test fun clearingRunsAfterTheWritesAskedBeforeItAndNothingComesBack() {
        val w = DraftWriter(store, exec)
        val copy = store.newCopy("in")
        val gate = java.util.concurrent.CountDownLatch(1)
        w.submit { gate.await(5, TimeUnit.SECONDS) }
        val a = w.write(draft(copy = copy.name), 1)
        val c = w.clear(copy.name)
        gate.countDown()
        a.get(5, TimeUnit.SECONDS)
        c.get(5, TimeUnit.SECONDS)
        assertEquals(ShareDraftStore.Read.Absent, store.read())
        assertFalse(copy.exists())
    }

    @Test fun theLastWriteIsWaitedForAtMostAShortTime() {
        assertEquals(300L, ShareDraftRules.STOP_WAIT_MS)
        val slow = java.util.concurrent.FutureTask<ShareDraftStore.Wrote> { Thread.sleep(2_000); ShareDraftStore.Wrote.OK }
        Thread(slow).apply { isDaemon = true }.start()
        val t0 = System.nanoTime()
        assertFalse(ShareDraftRules.durable(slow, ShareDraftRules.STOP_WAIT_MS))
        assertTrue((System.nanoTime() - t0) / 1_000_000 < 1_500)
        slow.cancel(true)
    }

    // ------------------------------------------------------------------ the one size limit

    private val emoji = "😀"

    @Test fun aMaximumBodyOfSupplementaryCharactersRoundTripsThroughTheFile() {
        // The most the draft holds: 600 000 characters = 300 000 emoji, and as many in the notes' share.
        val body = emoji.repeat(300_000)
        val d = draft(text = body).copy(notes = emoji.repeat(50_000), saving = marker)
        val bytes = ShareDraftRules.encodeBytes(d)
        // Raw UTF-8: 4 bytes an emoji, no escapes, and the body once.
        assertTrue(bytes.size < 1_500_000 + 400_000)
        val text = String(bytes, Charsets.UTF_8)
        assertFalse(text.contains("\\uD83D"))
        assertFalse(text.contains("\\ud83d"))
        assertTrue(text.indexOf(body) >= 0)
        assertEquals(text.indexOf(body), text.lastIndexOf(body))
        assertTrue(bytes.size <= ShareDraftRules.MAX_SERIALIZED_BYTES)
        // Through the file and back: the reader accepts what the writer wrote.
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(d, 1))
        assertEquals(d, onDisk())
        assertTrue(ShareDraftRules.decode(text) is ShareDraftRules.Decoded.Ok)
    }

    @Test fun theBodyOfTheIntakeMaximumInEmojiIsKept() {
        // The intake's own maximum, in bytes: 500 KB of 4-byte characters.
        val body = emoji.repeat(AttachLimits.maxTextBytes / 4)
        val d = draft(text = body)
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(d, 1))
        assertEquals(body, onDisk().text)
    }

    @Test fun whatTheWriterCanProduceAtTheWorstIsBelowTheLimitOfTheReader() {
        assertTrue(ShareDraftRules.worstCaseBytes() < ShareDraftRules.MAX_SERIALIZED_BYTES)
        // Controls are the widest characters (six bytes escaped): a body of them is the worst case and still reads.
        val d = draft(text = "\u0001".repeat(600_000)).copy(notes = "\u0002".repeat(100_000))
        val bytes = ShareDraftRules.encodeBytes(d)
        assertTrue(bytes.size <= ShareDraftRules.worstCaseBytes())
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(d, 1))
        assertEquals(d, onDisk())
    }

    @Test fun aLoneSurrogateAndEveryControlStillSurviveWhileTheRestIsRaw() {
        val d = draft(text = "a\uD83Db \uDE00 c\u0000d\u001Fe\u007Ff g\"h\\i\n\r\tj $emoji")
        val text = ShareDraftRules.encode(d)
        assertFalse(text.any { it < ' ' })
        assertTrue(text.contains(emoji))
        assertTrue(text.contains("\\ud83d") || text.contains("\\uD83D") || text.contains("\\ud83D"))
        assertEquals(d, (ShareDraftRules.decode(text) as ShareDraftRules.Decoded.Ok).draft)
    }

    @Test fun aDraftTooBigToKeepFailsTheWriteAndLeavesTheOldOne() {
        val small = ShareDraftStore(File(dir, "pequeno"), maxBytes = 2_000)
        assertEquals(ShareDraftStore.Wrote.OK, small.writeRevision(draft(text = "x"), 1))
        assertEquals(ShareDraftStore.Wrote.FAILED, small.writeRevision(draft(text = "y".repeat(5_000)), 2))
        assertEquals("x", ((small.read() as ShareDraftStore.Read.Found).draft).text)
        assertEquals(listOf(ShareDraftStore.FILE), File(dir, "pequeno").list()!!.toList())
        // And a file bigger than the limit is not a draft (unless it says it is a newer one).
        File(dir, "pequeno/${ShareDraftStore.FILE}").writeText("{\"v\":1,\"x\":\"" + "z".repeat(3_000) + "\"}")
        assertEquals(ShareDraftStore.Read.Corrupt, small.read())
        File(dir, "pequeno/${ShareDraftStore.FILE}").writeText("{\"v\":2,\"x\":\"" + "z".repeat(3_000) + "\"}")
        assertEquals(ShareDraftStore.Read.Newer, small.read())
    }

    @Test fun theDraftHoldsTheBodyOnceAndTheMarkerHoldsNoCopyOfIt() {
        val text = ShareDraftRules.encode(draft(text = "CORPO-ÚNICO", saving = marker))
        assertEquals(text.indexOf("CORPO-ÚNICO"), text.lastIndexOf("CORPO-ÚNICO"))
        assertFalse(text.contains("stext"))
        assertFalse(text.contains("sbase"))
    }

    // ------------------------------------------------------------------ the age comes before the hash

    @Test fun anExpiredDraftIsDroppedWithoutHashingItsCopy() {
        val copy = store.newCopy("in")
        copy.writeBytes(ByteArray(100))
        val d = draft(copy = copy.name, createdAt = now - 8 * day).copy(size = 100, hash = "abc")
        assertTrue(store.write(d))
        var hashed = 0
        val s = store.start(now, setUp = true) { hashed++; "abc" }
        assertEquals(ShareDraftRules.MSG_EXPIRED, (s as ShareDraftStore.Start.Dropped).message)
        assertEquals(0, hashed)
        assertFalse(copy.exists())
        assertEquals(ShareDraftStore.Read.Absent, store.read())
    }

    @Test fun aLiveDraftHasItsCopyHashedOnce() {
        val copy = store.newCopy("in")
        copy.writeBytes(ByteArray(100))
        val d = draft(copy = copy.name).copy(size = 100, hash = "abc")
        assertTrue(store.write(d))
        var hashed = 0
        val s = store.start(now, setUp = true) { hashed++; "abc" }
        assertTrue(s is ShareDraftStore.Start.Resume)
        assertEquals(1, hashed)
        assertTrue(ShareDraftRules.isExpired(d.copy(createdAt = now - 8 * day), now))
        assertFalse(ShareDraftRules.isExpired(d, now))
    }

    // ------------------------------------------------------------------ the format token and the quarantine

    @Test fun theVersionIsAWholeNumberTokenOrTheFileIsDamaged() {
        for (bad in listOf("{\"v\":2garbage", "{\"v\":02,\"x\":1}", "{\"v\":2.5,\"x\":1}", "{\"v\":2x}", "{\"v\":-3,\"x\":1}")) {
            assertTrue(bad, ShareDraftRules.decode(bad) is ShareDraftRules.Decoded.Corrupt)
        }
        for (newer in listOf("{\"v\":2}", "{\"v\":2,", "{ \"v\" : 12 ,\"x\":{{", "{\"v\":999999999,\"a\":[")) {
            assertTrue(newer, ShareDraftRules.decode(newer) is ShareDraftRules.Decoded.Newer)
        }
        assertFalse(ShareDraftRules.looksNewer("{\"v\":1,\"x\":1}"))
        // The damaged one is deleted at start (and told), not kept as "newer".
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":2garbage")
        val s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_LOST, (s as ShareDraftStore.Start.Dropped).message)
        assertFalse(File(dir, "rascunho/${ShareDraftStore.QUARANTINE}").exists())
    }

    @Test fun aNewerDraftIsQuarantinedSoTheSlotTakesANewDraft() {
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":2,\"copia\":\"x.bin\"}")
        File(dir, "rascunho/x.bin").writeBytes(ByteArray(5))
        val s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_NEWER, (s as ShareDraftStore.Start.Dropped).message)
        // The slot is free: a new draft is written and comes back at the next start.
        assertEquals(ShareDraftStore.Wrote.OK, store.writeRevision(draft(title = "novo"), 1))
        val again = store.start(now, setUp = true)
        assertEquals("novo", (again as ShareDraftStore.Start.Resume).draft.title)
        // The quarantine survives that, and the sweep of strays never touches it.
        val boxes = File(dir, "rascunho/${ShareDraftStore.QUARANTINE}").listFiles()!!
        assertEquals(1, boxes.size)
        assertTrue(File(boxes[0], "x.bin").exists())
        store.sweep(null)
        assertTrue(File(boxes[0], "x.bin").exists())
    }

    @Test fun theQuarantineIsSweptAfterThirtyDaysAndNotBefore() {
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":3}")
        store.start(now, setUp = true)
        val q = File(dir, "rascunho/${ShareDraftStore.QUARANTINE}")
        assertEquals(1, q.listFiles()!!.size)
        store.sweepQuarantine(now + 29 * day)
        assertEquals(1, q.listFiles()!!.size)
        store.start(now + 30 * day, setUp = true)
        assertEquals(1, q.listFiles()!!.size)
        store.start(now + 31 * day, setUp = true)
        assertFalse(q.exists())
        assertEquals(30 * day, ShareDraftRules.QUARANTINE_MS)
    }

    @Test fun twoNewerDraftsInARowGoToTwoPlaces() {
        File(dir, "rascunho").mkdirs()
        for (i in 1..2) {
            File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":2,\"n\":$i}")
            store.start(now, setUp = true)
        }
        val boxes = File(dir, "rascunho/${ShareDraftStore.QUARANTINE}").listFiles()!!
        assertEquals(2, boxes.size)
        assertEquals(setOf("{\"v\":2,\"n\":1}", "{\"v\":2,\"n\":2}"), boxes.map { File(it, ShareDraftStore.FILE).readText() }.toSet())
    }

    @Test fun wipingForAResetTakesTheQuarantineToo() {
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":2}")
        store.start(now, setUp = true)
        store.wipe()
        assertFalse(File(dir, "rascunho").exists())
    }
}
