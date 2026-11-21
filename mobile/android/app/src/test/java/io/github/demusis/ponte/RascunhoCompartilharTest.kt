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
        DraftSaving(op, "0a1b2c3d-0000-4000-8000-123456789abc", now - 100)

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
        assertTrue(ShareDraftRules.decode(good.removeSuffix("}") + ",\"sop\":\"voar\",\"sat\":1,\"sopid\":\"$opId\"}") is ShareDraftRules.Decoded.Corrupt)
        // An attach marker on a TEXT draft (no file to attach) is not a draft this app writes.
        val attachOnText = ShareDraftRules.encode(draft(copy = "", saving = marker(ShareDraftRules.OP_ATTACH)))
        assertTrue(ShareDraftRules.decode(attachOnText) is ShareDraftRules.Decoded.Corrupt)
        // But an empty target on the FORM is never a reason: the marker is judged by its own fields.
        val attachNoTarget = ShareDraftRules.encode(draft(saving = marker(ShareDraftRules.OP_ATTACH)).copy(targetId = 0, targetNumber = ""))
        assertTrue(ShareDraftRules.decode(attachNoTarget) is ShareDraftRules.Decoded.Ok)
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
        f.writeBytes(ByteArray(9 * 1024 * 1024) { 'a'.code.toByte() })
        assertEquals(ShareDraftStore.Read.Corrupt, store.read())
    }

    @Test fun aNewerVersionIsNotCorruptAndIsPutAsideWithItsCopyNotDeleted() {
        File(dir, "rascunho").mkdirs()
        val newer = "{\"v\":2,\"cria\":{\"quando\":1},\"itens\":[1,2,3],\"copia\":\"qualquer.bin\"}"
        val f = File(dir, "rascunho/${ShareDraftStore.FILE}")
        f.writeText(newer)
        val copyOfTheNewer = writeCopy("qualquer.bin")
        val stray = writeCopy("solto.bin")
        assertEquals(ShareDraftStore.Read.Newer, store.read())
        val s = store.start(now, setUp = true)
        assertEquals(ShareDraftRules.MSG_NEWER, (s as ShareDraftStore.Start.Dropped).message)
        // Out of the active slot, but every byte kept in the quarantine for the newer version.
        assertFalse(f.exists())
        val boxes = File(dir, "rascunho/${ShareDraftStore.QUARANTINE}").listFiles()!!
        assertEquals(1, boxes.size)
        assertEquals(newer, File(boxes[0], ShareDraftStore.FILE).readText())
        assertTrue(File(boxes[0], "qualquer.bin").exists())
        assertTrue(File(boxes[0], "solto.bin").exists())
        assertFalse(copyOfTheNewer.exists())
        assertTrue(stray.name.isNotEmpty())
        assertTrue(ShareDraftRules.decode(newer) is ShareDraftRules.Decoded.Newer)
        // A bigger number with a structure this reader cannot parse is still "newer", never "corrupt".
        assertTrue(ShareDraftRules.decode("{\"v\":7,\"x\":{{{") is ShareDraftRules.Decoded.Newer)
        assertTrue(ShareDraftRules.decode("{ \"v\" : 3 , \"x\":1}") is ShareDraftRules.Decoded.Newer)
        assertTrue(ShareDraftRules.decode("{\"v\":2}") is ShareDraftRules.Decoded.Newer)
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

    private val opId = "0a1b2c3d-0000-4000-8000-123456789abc"

    @Test fun theMarkerGoesEditingToSavingAndBackOnAFailure() {
        val d = draft()
        assertNull(d.saving)
        val saving = ShareDraftRules.begin(d, ShareDraftRules.OP_RECORD, opId, now)
        assertEquals(ShareDraftRules.markerFor(d, ShareDraftRules.OP_RECORD, opId, now), saving.saving)
        assertEquals(ShareDraftRules.fingerprint(d, ShareDraftRules.OP_RECORD), saving.saving!!.fingerprint)
        assertTrue(saving.saving!!.usable)
        // The marker is part of what is written: it survives the disk, with its operation id.
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

    // ------------------------------------------------------------------ recovery: only the core's word ends a draft

    private fun saved(r: ShareDraftRules.Recovery) = r as ShareDraftRules.Recovery.Saved
    private fun form(r: ShareDraftRules.Recovery) = r as ShareDraftRules.Recovery.Form

    @Test fun anOperationTheCoreCommittedEndsTheDraftAndOpensTheDocument() {
        val d = draft()
        fun sent(op: String) = d.copy(saving = ShareDraftRules.markerFor(d, op, opId, now))
        val r = ShareDraftRules.recover(sent(ShareDraftRules.OP_RECORD), ShareDraftRules.OperationStatus.Done(91))
        assertEquals(91L, saved(r).documentId)
        // An attach whose answer carries no document id opens the document it was aimed at (the marker's own).
        val attach = ShareDraftRules.recover(sent(ShareDraftRules.OP_ATTACH), ShareDraftRules.OperationStatus.Done(0))
        assertEquals(77L, saved(attach).documentId)
        // A record with no id in the answer: nothing to open, but still over.
        assertEquals(0L, saved(ShareDraftRules.recover(sent(ShareDraftRules.OP_RECORD), ShareDraftRules.OperationStatus.Done(0))).documentId)
    }

    @Test fun anOperationNotCommittedDropsTheMarkerAndTheSameContentMayReuseItsId() {
        val d = draft()
        val m = ShareDraftRules.markerFor(d, ShareDraftRules.OP_RECORD, opId, now)
        val f = form(ShareDraftRules.recover(d.copy(saving = m), ShareDraftRules.OperationStatus.NotDone))
        assertEquals(m, f.reuse)
        assertTrue(f.keep.isEmpty())
        assertFalse(f.unconfirmed)
        assertNull(f.earlier)
        // A marker that does not know what it was sent with is dropped too, and never reused.
        val legacy = form(ShareDraftRules.recover(draft(saving = marker()), ShareDraftRules.OperationStatus.NotDone))
        assertNull(legacy.reuse)
        assertTrue(legacy.keep.isEmpty())
    }

    @Test fun anUnreadableStatusKeepsTheDraftAndTheMarkerAndShowsTheFormWithANote() {
        for (status in listOf(ShareDraftRules.OperationStatus.Unknown, null)) {
            val f = form(ShareDraftRules.recover(draft(saving = marker()), status))
            assertTrue(f.unconfirmed)
            // The marker stays, so the next start asks again.
            assertEquals(listOf(marker()), f.keep)
            assertNull(f.reuse)
        }
    }

    @Test fun aMarkerWithNoUsableIdIsNeverResolvedByAGuess() {
        // What the first build wrote: a marker whose id never reached the core, plus fields only a guess could use.
        val legacyText = ShareDraftRules.encode(draft(copy = "")).removeSuffix("}") +
            ",\"sop\":\"record\",\"sid\":\"$opId\",\"sbase\":41,\"sat\":$now,\"stext\":\"Corpo\"}"
        val legacy = (ShareDraftRules.decode(legacyText) as ShareDraftRules.Decoded.Ok).draft
        assertNotNull(legacy.saving)
        assertFalse(legacy.saving!!.usable)
        // Whatever the core would say, the draft is the form, with the note, nothing deleted.
        for (status in listOf(ShareDraftRules.OperationStatus.Done(5), ShareDraftRules.OperationStatus.NotDone, null)) {
            val f = form(ShareDraftRules.recover(legacy, status))
            assertTrue(f.unconfirmed)
            assertNull(f.reuse)
            assertTrue(f.keep.isEmpty())
            assertNull(f.earlier)
        }
        // And it is a draft that is kept by the start, not dropped.
        File(dir, "rascunho").mkdirs()
        File(dir, "rascunho/${ShareDraftStore.FILE}").writeText(legacyText)
        val s = store.start(now, setUp = true)
        assertTrue(s is ShareDraftStore.Start.Resume)
        assertTrue((s as ShareDraftStore.Start.Resume).checkFirst)
        // A marker whose id is not a UUID is the same.
        val junk = ShareDraftRules.encode(draft(copy = "", saving = marker().copy(operationId = "não-é-uuid")))
        assertFalse((ShareDraftRules.decode(junk) as ShareDraftRules.Decoded.Ok).draft.saving!!.usable)
    }

    @Test fun aDraftThatWasOnlyBeingEditedIsTheFormWhateverTheCoreSays() {
        for (status in listOf(ShareDraftRules.OperationStatus.Done(9), ShareDraftRules.OperationStatus.NotDone, null)) {
            val f = form(ShareDraftRules.recover(draft(), status))
            assertNull(f.reuse)
            assertFalse(f.unconfirmed)
            assertTrue(f.keep.isEmpty())
            assertNull(f.earlier)
        }
    }

    @Test fun theCoreAnswerIsReadIntoItsThreeCases() {
        assertTrue(ShareDraftRules.statusFrom(true, true, 12) is ShareDraftRules.OperationStatus.Done)
        assertEquals(12L, (ShareDraftRules.statusFrom(true, true, 12) as ShareDraftRules.OperationStatus.Done).documentId)
        assertEquals(ShareDraftRules.OperationStatus.NotDone, ShareDraftRules.statusFrom(true, false, 0))
        // An error, an answer with no "done" (an older core) and the like are "not known", never "not done".
        assertEquals(ShareDraftRules.OperationStatus.Unknown, ShareDraftRules.statusFrom(false, true, 12))
        assertEquals(ShareDraftRules.OperationStatus.Unknown, ShareDraftRules.statusFrom(false, null, 0))
        assertEquals(ShareDraftRules.OperationStatus.Unknown, ShareDraftRules.statusFrom(true, null, 0))
    }

    @Test fun noRuleGuessesFromNamesSizesOrTextsAnymore() {
        // The unrelated "scan.pdf" of the same size on the target document, or an older record with the same text,
        // must never end a draft: there is no function left that compares them.
        val names = ShareDraftRules::class.java.methods.map { it.name }.toSet()
        for (gone in listOf("sameRecord", "alreadyAttached", "candidateIds", "afterDeath")) assertFalse(gone, gone in names)
        // The same name and size in "files of the target" changes nothing: only the core's operation status counts.
        val attach = draft(saving = marker(ShareDraftRules.OP_ATTACH))
        assertTrue(ShareDraftRules.recover(attach, ShareDraftRules.OperationStatus.NotDone) is ShareDraftRules.Recovery.Form)
        assertTrue(ShareDraftRules.recover(attach, null) is ShareDraftRules.Recovery.Form)
    }

    // ------------------------------------------------------------------ the operation id of a save

    private fun pending(id: String = opId, fp: String = "f", op: String = ShareDraftRules.OP_RECORD) =
        ShareDraftRules.PendingOp(op, id, fp)

    @Test fun theSameFormSavedAgainReusesItsOperationIdAndAChangedOneGetsANewId() {
        val same = ShareDraftRules.operationFor(pending(), ShareDraftRules.OP_RECORD, "f") { "NOVO" }
        assertEquals(opId, same.id)
        assertEquals("NOVO", ShareDraftRules.operationFor(pending(), ShareDraftRules.OP_RECORD, "outro") { "NOVO" }.id)
        assertEquals("NOVO", ShareDraftRules.operationFor(pending(), ShareDraftRules.OP_ATTACH, "f") { "NOVO" }.id)
        assertEquals("NOVO", ShareDraftRules.operationFor(null, ShareDraftRules.OP_RECORD, "f") { "NOVO" }.id)
        assertEquals("NOVO", ShareDraftRules.operationFor(pending(id = "x"), ShareDraftRules.OP_RECORD, "f") { "NOVO" }.id)
    }

    @Test fun theFingerprintFollowsWhatTheCallIsMadeOfAndNothingElse() {
        val d = draft(copy = "")
        val op = ShareDraftRules.OP_RECORD
        val fp = ShareDraftRules.fingerprint(d, op)
        // What does not go to the core does not count: the subject, the time, the target of a RECORD.
        assertEquals(fp, ShareDraftRules.fingerprint(d.copy(subject = "outro assunto", updatedAt = 1, targetId = 78, description = "x"), op))
        assertFalse(fp == ShareDraftRules.fingerprint(d.copy(title = "outro"), op))
        assertFalse(fp == ShareDraftRules.fingerprint(d.copy(mode = "RECORD"), op))
        assertFalse(fp == ShareDraftRules.fingerprint(d.copy(kind = "outro"), op))
        assertFalse(fp == ShareDraftRules.fingerprint(d, ShareDraftRules.OP_ATTACH))
        // An attach is made of the document (id and number), the description and the file's identity.
        val file = draft()
        val afp = ShareDraftRules.fingerprint(file, ShareDraftRules.OP_ATTACH)
        assertEquals(afp, ShareDraftRules.fingerprint(file.copy(title = "outro", notes = "n", kind = "x"), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(targetId = 78), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(targetNumber = "SEI 0413/2026"), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(description = "x"), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(copy = "in-2.bin"), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(name = "outro.pdf"), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(size = 1), ShareDraftRules.OP_ATTACH))
        assertFalse(afp == ShareDraftRules.fingerprint(file.copy(hash = "ffff"), ShareDraftRules.OP_ATTACH))
        // A restored draft fingerprints the same as the call it was saved with.
        val back = (ShareDraftRules.decode(ShareDraftRules.encode(d)) as ShareDraftRules.Decoded.Ok).draft
        assertEquals(fp, ShareDraftRules.fingerprint(back, op))
    }

    // ------------------------------------------------------------------ the document an attach is aimed at

    @Test fun aTargetWhoseNumberChangedIsToBeChosenAgain() {
        assertEquals(ShareDraftRules.TargetCheck.OK, ShareDraftRules.targetCheck("MANUAL-2026/0003", "MANUAL-2026/0003"))
        assertEquals(ShareDraftRules.TargetCheck.CHANGED, ShareDraftRules.targetCheck("MANUAL-2026/0003", "MANUAL-2026/0009"))
        assertEquals(ShareDraftRules.TargetCheck.CHANGED, ShareDraftRules.targetCheck("MANUAL-2026/0003", ""))
        // Could not be read, or no number kept: not known; the core's own check at save time decides.
        assertEquals(ShareDraftRules.TargetCheck.UNKNOWN, ShareDraftRules.targetCheck("MANUAL-2026/0003", null))
        assertEquals(ShareDraftRules.TargetCheck.UNKNOWN, ShareDraftRules.targetCheck("", "MANUAL-2026/0003"))
    }

    @Test fun theTargetNumberIsKeptWithTheDraftAndTheCoreCodeIsRecognised() {
        val back = (ShareDraftRules.decode(ShareDraftRules.encode(draft())) as ShareDraftRules.Decoded.Ok).draft
        assertEquals("SEI 0412/2026", back.targetNumber)
        assertTrue(ShareDraftRules.isTargetChanged("target_changed"))
        assertFalse(ShareDraftRules.isTargetChanged(""))
        assertFalse(ShareDraftRules.isTargetChanged(null))
        assertFalse(ShareDraftRules.isTargetChanged("outro"))
        assertTrue(ShareDraftRules.MSG_TARGET_CHANGED.contains("escolha o documento de novo"))
    }

    @Test fun theCoreIsCalledOnlyAfterTheMarkerIsDurable() {
        val log = ArrayList<String>()
        val r = ShareDraftRules.callAfterMarker({ log += "marcador"; true }) { log += "núcleo"; 7 }
        assertEquals(7, r)
        assertEquals(listOf("marcador", "núcleo"), log)

        // The marker did not reach the disk: the core is never called and the person is told.
        var called = false
        try {
            ShareDraftRules.callAfterMarker({ false }) { called = true }
            org.junit.Assert.fail("devia recusar")
        } catch (e: PonteException) {
            assertEquals(ShareDraftRules.MSG_CANNOT_PREPARE, e.message)
        }
        assertFalse(called)
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
