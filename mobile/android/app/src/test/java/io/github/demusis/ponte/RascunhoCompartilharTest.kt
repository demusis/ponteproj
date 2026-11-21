package io.github.demusis.ponte

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files

/**
 * The draft of a share in progress: its format, the 7 days, the damaged copy, the file that is damaged or
 * from a newer app, the save marker (editing → saving → cleared) and the search for a record that a dead
 * process had already made. Plain functions and a temporary folder, no Android.
 */
class RascunhoCompartilharTest {
    private lateinit var dir: File
    private lateinit var store: ShareDraftStore
    private val day = 24L * 60 * 60 * 1000
    private val now = 1_800_000_000_000L

    @Before fun setUp() {
        dir = Files.createTempDirectory("rascunho").toFile()
        store = ShareDraftStore(File(dir, "rascunho"))
    }

    @After fun tearDown() {
        dir.deleteRecursively()
    }

    private val nasty = "Ofício nº 0412/2026 — “resposta” à Câmara\nlinha 2\r\nlinha 3\tcom tab\n" +
        "aspas \"duplas\" e 'simples', barra \\ e /, chaves {\"a\":[1,2]}, \u0001 controle,   separador, " +
        "emoji 😀 e meia-letra solta \uD83D fim"

    private fun draft(
        copy: String = "in-1.bin", saving: DraftSaving? = null, createdAt: Long = now - 1000
    ) = ShareDraft(
        createdAt = createdAt, updatedAt = now - 500, copy = copy, name = "Ofício 0412 — resposta.pdf",
        size = 1_250_000L, mime = "application/pdf", hash = "ab12", text = if (copy.isEmpty()) nasty else "",
        subject = "Assunto: “urgente”", mode = "ATTACH", kind = "oficio", title = nasty, date = "2026-11-06",
        origin = "Servidor A — Sec. de Finanças", notes = nasty, targetId = 77, targetNumber = "SEI 0412/2026",
        targetTitle = "Ofício “77”", description = nasty, saving = saving
    )

    private fun marker(op: String = ShareDraftRules.OP_RECORD) =
        DraftSaving(op, "0a1b2c3d-0000-4000-8000-123456789abc", 41L, now - 100, nasty)

    private fun writeCopy(name: String = "in-1.bin", bytes: ByteArray = ByteArray(10) { it.toByte() }): File {
        val f = store.newCopy("in")
        val target = File(f.parentFile, name)
        f.renameTo(target)
        target.writeBytes(bytes)
        return target
    }

    // ------------------------------------------------------------------ the format

    @Test fun everyFieldSurvivesTheRoundTripWithAccentsBreaksAndQuotes() {
        for (d in listOf(draft(), draft(copy = ""), draft(saving = marker()), draft(saving = marker(ShareDraftRules.OP_ATTACH)))) {
            val back = ShareDraftRules.decode(ShareDraftRules.encode(d))
            assertTrue(back is ShareDraftRules.Decoded.Ok)
            assertEquals(d, (back as ShareDraftRules.Decoded.Ok).draft)
        }
    }

    @Test fun theEncodedDraftIsOneValidJsonObjectStartingWithTheVersion() {
        val text = ShareDraftRules.encode(draft())
        assertTrue(text.startsWith("{\"v\":1,"))
        val m = FlatJson.read(text)!!
        assertEquals(1L, m["v"])
        assertEquals(nasty, m["title"])
        // Not one raw line break or control character in the file: all escaped.
        assertFalse(text.any { it < ' ' })
    }

    @Test fun anEmptyFieldAndALongOneComeBackAsTheyWere() {
        val d = draft().copy(title = "", notes = "x".repeat(50_000))
        val back = (ShareDraftRules.decode(ShareDraftRules.encode(d)) as ShareDraftRules.Decoded.Ok).draft
        assertEquals(d, back)
    }

    @Test fun aHugeFieldIsCutToWhatADraftMayHoldBeforeItIsWritten() {
        val d = draft().copy(title = "t".repeat(50_000), notes = "n".repeat(500_000))
        val back = (ShareDraftRules.decode(ShareDraftRules.encode(d)) as ShareDraftRules.Decoded.Ok).draft
        assertEquals(4_000, back.title.length)
        assertEquals(100_000, back.notes.length)
    }

    @Test fun aFieldThatTheDraftDoesNotKnowIsIgnoredInTheSameVersion() {
        val text = ShareDraftRules.encode(draft()).removeSuffix("}") + ",\"futuro\":\"x\",\"n\":3,\"b\":true,\"z\":null}"
        assertTrue(ShareDraftRules.decode(text) is ShareDraftRules.Decoded.Ok)
    }

    // ------------------------------------------------------------------ a damaged file is no draft; a newer one is left alone

    @Test fun damagedTextsAreCorruptAndNeverThrow() {
        val good = ShareDraftRules.encode(draft())
        val bad = listOf(
            "", "   ", "null", "[]", "{", "}", "{}", "lixo", "{\"v\":1}", "\u0000\u0000\u0000",
            good.substring(0, good.length / 2), good.substring(0, good.length - 1),
            good.replace("\"v\":1", "\"v\":\"1\""), good.replace("\"v\":1", "\"v\":0"),
            good.replace("\"size\":1250000", "\"size\":\"grande\""), good.replace("\"size\":1250000", "\"size\":-5"),
            good.replace("\"mode\":\"ATTACH\"", "\"mode\":\"OUTRO\""),
            good.replace("\"copy\":\"in-1.bin\"", "\"copy\":\"../../segredo\""),
            good.replace("\"copy\":\"in-1.bin\"", "\"copy\":\"a/b\""),
            good.replace("\"created\":${now - 1000}", "\"created\":0"),
            good.replace("\"title\"", "\"titulo\""),
            good + "{}", good + "x", good.replace("\"v\":1,", "\"v\":1,\"v\":1,"),
            good.replace("\"tid\":77", "\"tid\":{\"a\":1}"),
            "{\"v\":1,\"created\":1.5}"
        )
        for (t in bad) assertTrue("devia ser corrompido: $t", ShareDraftRules.decode(t) is ShareDraftRules.Decoded.Corrupt)
        assertTrue(ShareDraftRules.decode(ShareDraftRules.encode(draft(copy = "").copy(text = ""))) is ShareDraftRules.Decoded.Corrupt)
        // A save marker with an operation that does not exist, or an attach without a document.
        assertTrue(ShareDraftRules.decode(good.replace("}", ",\"sop\":\"voar\",\"sid\":\"1\",\"sbase\":0,\"sat\":1,\"stext\":\"\"}")) is ShareDraftRules.Decoded.Corrupt)
        val attachNoDoc = ShareDraftRules.encode(draft(saving = marker(ShareDraftRules.OP_ATTACH)).copy(targetId = 0))
        assertTrue(ShareDraftRules.decode(attachNoDoc) is ShareDraftRules.Decoded.Corrupt)
    }

    @Test fun aCorruptFileOnTheDiskIsNoDraftAndIsRemovedWithTheTellingDone() {
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText("{\"v\":1,\"created\":")
        val copy = writeCopy()
        assertEquals(ShareDraftStore.Read.Corrupt, store.read())
        val s = store.start(now, setUp = true)
        assertTrue(s is ShareDraftStore.Start.Dropped)
        assertEquals(ShareDraftRules.MSG_LOST, (s as ShareDraftStore.Start.Dropped).message)
        assertFalse(File(dir, "rascunho/${ShareDraftStore.FILE}").exists())
        assertFalse(copy.exists())
        // And the next start finds nothing.
        assertEquals(ShareDraftStore.Start.None, store.start(now, setUp = true))
    }

    @Test fun aBinaryOrHugeFileIsCorruptNotACrash() {
        File(dir, "rascunho").mkdirs()
        val f = File(dir, "rascunho/${ShareDraftStore.FILE}")
        f.writeBytes(ByteArray(5_000) { (it * 31).toByte() })
        assertEquals(ShareDraftStore.Read.Corrupt, store.read())
        f.writeBytes(ByteArray(5 * 1024 * 1024) { 'a'.code.toByte() })
        assertEquals(ShareDraftStore.Read.Corrupt, store.read())
    }

    @Test fun aNewerVersionIsNotCorruptAndIsLeftUntouchedWithItsCopy() {
        File(dir, "rascunho").mkdirs()
        val newer = "{\"v\":2,\"cria\":{\"quando\":1},\"itens\":[1,2,3],\"copia\":\"qualquer.bin\"}"
        val f = File(dir, "rascunho/${ShareDraftStore.FILE}")
        f.writeText(newer)
        val copyOfTheNewer = writeCopy("qualquer.bin")
        val stray = writeCopy("solto.bin")
        assertEquals(ShareDraftStore.Read.Newer, store.read())
        assertEquals(ShareDraftStore.Start.Blocked, store.start(now, setUp = true))
        // Untouched: the file as it was, and no sweep of what it may point to.
        assertEquals(newer, f.readText())
        assertTrue(copyOfTheNewer.exists())
        assertTrue(stray.exists())
        assertTrue(ShareDraftRules.decode(newer) is ShareDraftRules.Decoded.Newer)
        // A bigger number with a structure this reader cannot parse is still "newer", never "corrupt".
        assertTrue(ShareDraftRules.decode("{\"v\":7,\"x\":{{{") is ShareDraftRules.Decoded.Newer)
        assertTrue(ShareDraftRules.decode("{ \"v\" : 3 , \"x\":1}") is ShareDraftRules.Decoded.Newer)
    }

    // ------------------------------------------------------------------ 7 days

    @Test fun aDraftExpiresAfterSevenDaysFromTheDayTheShareArrived() {
        val d = draft(copy = "")
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d, d.createdAt + 7 * day, null))
        assertEquals(ShareDraftRules.Verdict.EXPIRED, ShareDraftRules.verdict(d, d.createdAt + 7 * day + 1, null))
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d, d.createdAt + 6 * day, null))
        // Editing it does not renew it: only the arrival counts.
        val touched = d.copy(updatedAt = d.createdAt + 6 * day)
        assertEquals(ShareDraftRules.Verdict.EXPIRED, ShareDraftRules.verdict(touched, d.createdAt + 8 * day, null))
        // An old draft is told as expired even when its copy is gone as well.
        assertEquals(ShareDraftRules.Verdict.EXPIRED, ShareDraftRules.verdict(draft(), now + 30 * day, null))
        // A clock that went back (created "in the future") does not expire anything.
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d.copy(createdAt = now + day), now, null))
    }

    @Test fun anExpiredDraftOnTheDiskIsRemovedWithItsCopyAndToldOnce() {
        val copy = writeCopy()
        val d = draft(createdAt = now - 8 * day).copy(size = copy.length(), hash = "")
        assertTrue(store.write(d))
        val s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_EXPIRED, (s as ShareDraftStore.Start.Dropped).message)
        assertFalse(copy.exists())
        assertEquals(ShareDraftStore.Read.Absent, store.read())
        assertEquals(ShareDraftStore.Start.None, store.start(now, setUp = true))
    }

    // ------------------------------------------------------------------ the copy

    @Test fun theCopyMustBeThereOfTheSameSizeAndHash() {
        val d = draft()
        assertEquals(ShareDraftRules.Verdict.COPY_LOST, ShareDraftRules.verdict(d, now, null))
        assertEquals(ShareDraftRules.Verdict.COPY_LOST, ShareDraftRules.verdict(d, now, CopyFacts(d.size - 1, "ab12")))
        assertEquals(ShareDraftRules.Verdict.COPY_LOST, ShareDraftRules.verdict(d, now, CopyFacts(d.size, "ffff")))
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d, now, CopyFacts(d.size, "ab12")))
        // No hash kept (or not worked out): the size alone decides.
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d.copy(hash = ""), now, CopyFacts(d.size, null)))
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(d, now, CopyFacts(d.size, null)))
        // A text has no copy to lose.
        assertEquals(ShareDraftRules.Verdict.READY, ShareDraftRules.verdict(draft(copy = ""), now, null))
    }

    @Test fun aMissingChangedOrTruncatedCopyDropsTheDraftAndTellsOnce() {
        val copy = writeCopy(bytes = ByteArray(1000) { it.toByte() })
        val d = draft().copy(size = 1000, hash = ShareDraftStore.hashOf(copy))
        assertTrue(store.write(d))
        // Intact: the draft comes back.
        assertTrue(store.start(now, setUp = true) is ShareDraftStore.Start.Resume)

        // Same size, one byte changed: the hash catches it.
        val bytes = copy.readBytes().also { it[500] = 99 }
        copy.writeBytes(bytes)
        var s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_LOST, (s as ShareDraftStore.Start.Dropped).message)
        assertFalse(copy.exists())
        assertEquals(ShareDraftStore.Read.Absent, store.read())

        // Truncated copy.
        val copy2 = writeCopy(bytes = ByteArray(1000) { it.toByte() })
        assertTrue(store.write(d))
        copy2.writeBytes(ByteArray(10))
        s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_LOST, (s as ShareDraftStore.Start.Dropped).message)

        // Copy deleted.
        val copy3 = writeCopy(bytes = ByteArray(1000) { it.toByte() })
        assertTrue(store.write(d))
        copy3.delete()
        s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_LOST, (s as ShareDraftStore.Start.Dropped).message)
        assertEquals(ShareDraftStore.Read.Absent, store.read())
    }

    @Test fun aDraftOfAPhoneThatIsNotSetUpAnymoreIsDroppedSilently() {
        val copy = writeCopy()
        assertTrue(store.write(draft().copy(size = copy.length(), hash = "")))
        val s = store.start(now, setUp = false)
        assertNull((s as ShareDraftStore.Start.Dropped).message)
        assertFalse(copy.exists())
    }

    @Test fun aTextDraftIsRestoredWithNoCopyAtAll() {
        assertTrue(store.write(draft(copy = "")))
        val s = store.start(now, setUp = true)
        assertTrue(s is ShareDraftStore.Start.Resume)
        assertEquals(draft(copy = "").text, (s as ShareDraftStore.Start.Resume).draft.text)
        assertFalse(s.checkFirst)
    }

    @Test fun theCopyNameCannotEscapeTheFolder() {
        for (bad in listOf("", ".", "..", "../x", "a/b", "a\\b", "x\u0000y", "a..b", "x".repeat(121))) {
            assertFalse(bad, ShareDraftRules.safeName(bad))
        }
        assertTrue(ShareDraftRules.safeName("in-123456.bin"))
        assertNull(store.facts("../x", withHash = false))
    }

    // ------------------------------------------------------------------ the save marker

    @Test fun theMarkerGoesEditingToSavingAndBackOnAFailure() {
        val d = draft()
        assertNull(d.saving)
        val saving = ShareDraftRules.begin(d, ShareDraftRules.OP_RECORD, "op-1", 41L, "texto", now)
        assertEquals(DraftSaving(ShareDraftRules.OP_RECORD, "op-1", 41L, now, "texto"), saving.saving)
        // The marker is part of what is written: it survives the disk.
        assertEquals(saving, (ShareDraftRules.decode(ShareDraftRules.encode(saving)) as ShareDraftRules.Decoded.Ok).draft)
        val rolled = ShareDraftRules.rollback(saving, now + 5)
        assertNull(rolled.saving)
        assertEquals(now + 5, rolled.updatedAt)
        assertEquals(d.title, rolled.title)
    }

    @Test fun aDraftThatDiedWhileSavingIsLookedAtBeforeTheFormComes() {
        val copy = writeCopy()
        val saving = draft().copy(size = copy.length(), hash = "", saving = marker())
        assertEquals(ShareDraftRules.Verdict.CHECK_FIRST, ShareDraftRules.verdict(saving, now, CopyFacts(copy.length(), null)))
        assertTrue(store.write(saving))
        val s = store.start(now, setUp = true) as ShareDraftStore.Start.Resume
        assertTrue(s.checkFirst)
        assertEquals(marker(), s.draft.saving)
    }

    @Test fun diedWhileSavingWithTheRecordAlreadyThereTheDraftIsOverAndNothingIsSavedAgain() {
        assertEquals(ShareDraftRules.Resolution.ALREADY_SAVED, ShareDraftRules.afterDeath(draft(saving = marker()), found = true))
    }

    @Test fun diedWhileSavingWithoutTheRecordTheFormIsBackReadyToSave() {
        assertEquals(ShareDraftRules.Resolution.SHOW_FORM, ShareDraftRules.afterDeath(draft(saving = marker()), found = false))
        // A draft that was only being edited is the form whatever the library holds.
        assertEquals(ShareDraftRules.Resolution.SHOW_FORM, ShareDraftRules.afterDeath(draft(), found = true))
        assertEquals(ShareDraftRules.Resolution.SHOW_FORM, ShareDraftRules.afterDeath(draft(), found = false))
    }

    private val docs = listOf(DocRow(40, true), DocRow(41, true), DocRow(42, false), DocRow(43, true), DocRow(50, true))

    @Test fun onlyManualRecordsNewerThanTheMarkerAreCandidatesNewestFirst() {
        assertEquals(listOf(50L, 43L), ShareDraftRules.candidateIds(41, docs))
        assertEquals(listOf(50L, 43L, 41L, 40L), ShareDraftRules.candidateIds(0, docs))
        assertEquals(emptyList<Long>(), ShareDraftRules.candidateIds(50, docs))
        assertEquals(2, ShareDraftRules.candidateIds(0, (1L..100L).map { DocRow(it, true) }, limit = 2).size)
    }

    @Test fun aFileRecordIsRecognisedByTheNameAndSizeOfItsAttachment() {
        val d = draft(saving = marker())
        val name = d.name
        assertTrue(ShareDraftRules.sameRecord(d, DocFacts(listOf(name to d.size), "qualquer")))
        assertTrue(ShareDraftRules.sameRecord(d, DocFacts(listOf("outro.txt" to 3L, name to d.size), "")))
        assertFalse(ShareDraftRules.sameRecord(d, DocFacts(listOf(name to d.size + 1), "")))
        assertFalse(ShareDraftRules.sameRecord(d, DocFacts(listOf("outro.pdf" to d.size), "")))
        assertFalse(ShareDraftRules.sameRecord(d, DocFacts(emptyList(), "")))
        // Without a marker nothing is "the record of a save".
        assertFalse(ShareDraftRules.sameRecord(draft(), DocFacts(listOf(name to d.size), "")))
    }

    @Test fun aTextRecordIsRecognisedByItsText() {
        val d = draft(copy = "", saving = marker().copy(text = "Linha um\nlinha dois"))
        assertTrue(ShareDraftRules.sameRecord(d, DocFacts(emptyList(), "Linha um\nlinha dois")))
        assertTrue(ShareDraftRules.sameRecord(d, DocFacts(emptyList(), "  Linha um\r\nlinha dois \n")))
        assertFalse(ShareDraftRules.sameRecord(d, DocFacts(emptyList(), "Linha um")))
        assertFalse(ShareDraftRules.sameRecord(d, DocFacts(emptyList(), "")))
        // An empty expected text never matches everything.
        assertFalse(ShareDraftRules.sameRecord(draft(copy = "", saving = marker().copy(text = "")), DocFacts(emptyList(), "")))
    }

    @Test fun anAttachIsAlreadyDoneWhenTheDocumentHoldsTheSameNameAndSize() {
        val d = draft(saving = marker(ShareDraftRules.OP_ATTACH))
        assertTrue(ShareDraftRules.alreadyAttached(d, listOf("x.txt" to 1L, d.name to d.size)))
        assertFalse(ShareDraftRules.alreadyAttached(d, listOf(d.name to d.size + 1)))
        assertFalse(ShareDraftRules.alreadyAttached(d, emptyList()))
    }

    // ------------------------------------------------------------------ the disk

    @Test fun theDraftIsWrittenAtomicallyAndReplacedWithoutLeavingATemporary() {
        val d = draft(copy = "")
        assertTrue(store.write(d))
        assertTrue(store.write(d.copy(title = "outro título")))
        val back = store.read() as ShareDraftStore.Read.Found
        assertEquals("outro título", back.draft.title)
        assertEquals(listOf(ShareDraftStore.FILE), File(dir, "rascunho").list()!!.toList())
    }

    @Test fun aWriteThatFailsLeavesTheOldDraftAndSaysSo() {
        val d = draft(copy = "")
        assertTrue(store.write(d))
        // The temporary name is taken by a folder: the write cannot happen (as with a full disk).
        File(dir, "rascunho/${ShareDraftStore.TMP}").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.TMP}/x").writeText("x")
        assertFalse(store.write(d.copy(title = "novo")))
        assertEquals(d.title, (store.read() as ShareDraftStore.Read.Found).draft.title)
        // And where the folder cannot even be made, nothing throws.
        val blocked = File(dir, "arquivo").apply { writeText("x") }
        assertFalse(ShareDraftStore(File(blocked, "dentro")).write(d))
    }

    @Test fun clearingRemovesTheDraftAndItsCopy() {
        val copy = writeCopy()
        assertTrue(store.write(draft().copy(size = copy.length(), hash = "")))
        store.clear(copy.name)
        assertEquals(ShareDraftStore.Read.Absent, store.read())
        assertFalse(copy.exists())
        // Twice is fine.
        store.clear(copy.name)
        store.clear(null)
        store.clear("../fora")
    }

    @Test fun theCleanupNeverDeletesTheCopyTheDraftPointsTo() {
        val live = writeCopy("in-live.bin")
        val orphan = writeCopy("in-orfa.bin")
        val half = File(dir, "rascunho/${ShareDraftStore.TMP}").apply { writeText("{") }
        assertTrue(store.write(draft(copy = live.name).copy(size = live.length(), hash = "")))
        store.sweep(live.name)
        assertTrue(live.exists())
        assertTrue(File(dir, "rascunho/${ShareDraftStore.FILE}").exists())
        assertFalse(orphan.exists())
        assertFalse(half.exists())
        // The start does it: the draft is restored and the orphan goes, the live copy stays.
        val orphan2 = writeCopy("in-orfa2.bin")
        val s = store.start(now, setUp = true)
        assertTrue(s is ShareDraftStore.Start.Resume)
        assertTrue(live.exists())
        assertFalse(orphan2.exists())
        // The rule the other cleaners use.
        assertFalse(ShareDraftRules.mayDelete(live.absolutePath, setOf(live.absolutePath)))
        assertTrue(ShareDraftRules.mayDelete(orphan.absolutePath, setOf(live.absolutePath)))
        // And what lies inside the draft folder is recognised, for a cleaner of another folder.
        assertTrue(ShareDraftRules.insideFolder(live.absolutePath, File(dir, "rascunho").absolutePath))
        assertFalse(ShareDraftRules.insideFolder(File(dir, "rascunho-outro/x").absolutePath, File(dir, "rascunho").absolutePath))
    }

    @Test fun withNoDraftEverythingLooseInTheFolderIsSwept() {
        val a = writeCopy("in-a.bin")
        val b = writeCopy("in-b.bin")
        assertEquals(ShareDraftStore.Start.None, store.start(now, setUp = true))
        assertFalse(a.exists())
        assertFalse(b.exists())
    }

    @Test fun wipingForAResetRemovesTheWholeFolder() {
        val copy = writeCopy()
        assertTrue(store.write(draft().copy(size = copy.length(), hash = "")))
        File(dir, "rascunho/${ShareDraftStore.TMP}").writeText("{")
        store.wipe()
        assertFalse(File(dir, "rascunho").exists())
        assertEquals(ShareDraftStore.Read.Absent, store.read())
        store.wipe()
    }

    @Test fun theHashOfTheCopyIsTheSha256InHex() {
        val f = File(dir, "h.bin").apply { writeText("abc") }
        assertEquals("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", ShareDraftStore.hashOf(f))
    }

    // ------------------------------------------------------------------ the form that comes back

    @Test fun aModeThatCannotBeShownFallsBackToTheFirstStep() {
        assertEquals("RECORD", ShareDraftRules.normalMode("RECORD", hasFile = false, hasTarget = false))
        assertEquals("ATTACH", ShareDraftRules.normalMode("ATTACH", hasFile = true, hasTarget = true))
        assertEquals("CHOOSE", ShareDraftRules.normalMode("ATTACH", hasFile = false, hasTarget = true))
        assertEquals("CHOOSE", ShareDraftRules.normalMode("ATTACH", hasFile = true, hasTarget = false))
        assertEquals("CHOOSE", ShareDraftRules.normalMode("CHOOSE", hasFile = true, hasTarget = true))
    }

    @Test fun theTextsToldToThePersonAreTheOnesAsked() {
        assertEquals("Rascunho recuperado.", ShareDraftRules.MSG_RECOVERED)
        assertEquals("O item recebido não pôde ser recuperado; compartilhe de novo.", ShareDraftRules.MSG_LOST)
        assertTrue(ShareDraftRules.MSG_EXPIRED.contains("7 dias"))
        assertNotNull(ShareDraftRules.MSG_CANNOT_KEEP)
    }

    @Test fun theRetentionIsSevenDays() {
        assertEquals(7 * day, ShareDraftRules.MAX_AGE_MS)
        assertEquals("rascunho", ShareDraftStore.DIR)
    }
}
