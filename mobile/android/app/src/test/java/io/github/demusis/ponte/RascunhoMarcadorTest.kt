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
 * The save marker bound to what it was sent with, and what the core's word about it means for the draft as it is
 * NOW: the state table (status x edited?), the kind of the operation, saving again while a save is unresolved,
 * several unresolved saves side by side, and the draft file that carries them (also the one an earlier build wrote).
 */
class RascunhoMarcadorTest {
    private val now = 1_800_000_000_000L
    private val id1 = "0a1b2c3d-0000-4000-8000-123456789abc"
    private val id2 = "1a1b2c3d-0000-4000-8000-123456789abc"
    private val id3 = "2a1b2c3d-0000-4000-8000-123456789abc"
    private val rec = ShareDraftRules.OP_RECORD
    private val att = ShareDraftRules.OP_ATTACH
    private val done = ShareDraftRules.OperationStatus.Done(91)
    private val notDone = ShareDraftRules.OperationStatus.NotDone
    private val unknown = ShareDraftRules.OperationStatus.Unknown

    private lateinit var dir: File
    private lateinit var store: ShareDraftStore

    @Before fun setUp() {
        dir = Files.createTempDirectory("marcador").toFile()
        store = ShareDraftStore(File(dir, "rascunho"))
    }

    @After fun tearDown() {
        dir.deleteRecursively()
    }

    /** A record being filled in from a shared text. */
    private fun text(title: String = "Ofício 0412") = ShareDraft(
        createdAt = now - 1000, updatedAt = now - 500, copy = "", name = "", size = 0, mime = "text/plain", hash = "",
        text = "Corpo do texto", subject = "", mode = "RECORD", kind = "oficio", title = title, date = "2026-11-06",
        origin = "Servidor A", notes = "", targetId = 0, targetNumber = "", targetTitle = "", description = "", saving = null
    )

    /** A file chosen to be attached to document 77. */
    private fun file(description: String = "") = ShareDraft(
        createdAt = now - 1000, updatedAt = now - 500, copy = "in-1.bin", name = "scan.pdf", size = 1000, mime = "application/pdf",
        hash = "ab12", text = "", subject = "", mode = "ATTACH", kind = "outro", title = "scan", date = "", origin = "",
        notes = "", targetId = 77, targetNumber = "SEI 0412/2026", targetTitle = "Ofício", description = description, saving = null
    )

    private fun sent(d: ShareDraft, op: String, id: String = id1) = ShareDraftRules.markerFor(d, op, id, now)
    private fun withMarker(d: ShareDraft, op: String, id: String = id1) = d.copy(saving = sent(d, op, id))
    private fun form(r: ShareDraftRules.Recovery) = r as ShareDraftRules.Recovery.Form
    private fun saved(r: ShareDraftRules.Recovery) = r as ShareDraftRules.Recovery.Saved

    private fun roundTrip(d: ShareDraft) =
        (ShareDraftRules.decode(ShareDraftRules.encode(d)) as ShareDraftRules.Decoded.Ok).draft

    // ------------------------------------------------------------------ the state table: status x edited?

    @Test fun doneAndTheSameContentConfirmsTheSaveAndClearsTheDraft() {
        val d = text()
        val r = ShareDraftRules.recover(withMarker(d, rec), listOf(done))
        assertEquals(91L, saved(r).documentId)
        // Also after the marker went through the disk: the fingerprint is the one of the content that was sent.
        assertEquals(91L, saved(ShareDraftRules.recover(roundTrip(withMarker(d, rec)), listOf(done))).documentId)
    }

    @Test fun doneButEditedAfterwardsKeepsTheEditsDropsTheMarkerAndSetsTheNote() {
        val m = sent(text(), rec)
        val edited = text(title = "Ofício 0412 — corrigido").copy(saving = m)
        val f = form(ShareDraftRules.recover(edited, listOf(done)))
        assertTrue(f.keep.isEmpty())
        assertNull(f.reuse)
        assertFalse(f.unconfirmed)
        assertEquals(91L, f.earlier!!.documentId)
        val note = ShareDraftRules.earlierNote(f.earlier!!.documentId, f.earlier!!.number)
        assertEquals("A versão anterior deste item já tinha sido salva (documento 91). Estas alterações ainda não foram salvas.", note)
        // An attach names the document by its number.
        val a = file()
        val fa = form(ShareDraftRules.recover(a.copy(description = "depois", saving = sent(a, att)), listOf(ShareDraftRules.OperationStatus.Done(0))))
        assertEquals(77L, fa.earlier!!.documentId)
        assertEquals("SEI 0412/2026", fa.earlier!!.number)
        assertTrue(ShareDraftRules.earlierNote(77, "SEI 0412/2026").contains("(documento SEI 0412/2026)"))
        assertFalse(ShareDraftRules.earlierNote(0, "").contains("("))
    }

    @Test fun notDoneDropsTheMarkerAndOnlyTheSameContentReusesTheId() {
        val d = text()
        val m = sent(d, rec)
        val same = form(ShareDraftRules.recover(d.copy(saving = m), listOf(notDone)))
        assertEquals(m, same.reuse)
        assertTrue(same.keep.isEmpty())
        assertFalse(same.unconfirmed)
        // Edited: the marker is dropped too, but its id is not reused for other content.
        val edited = form(ShareDraftRules.recover(text(title = "outro título").copy(saving = m), listOf(notDone)))
        assertNull(edited.reuse)
        assertTrue(edited.keep.isEmpty())
        assertNull(edited.earlier)
    }

    @Test fun unknownKeepsTheMarkerWhetherOrNotTheFormWasEdited() {
        val m = sent(text(), rec)
        for (d in listOf(text(), text(title = "editado"))) {
            for (st in listOf(unknown, null)) {
                val f = form(ShareDraftRules.recover(d.copy(saving = m), listOf(st)))
                assertEquals(listOf(m), f.keep)
                assertTrue(f.unconfirmed)
                assertNull(f.earlier)
                assertNull(f.reuse)
            }
        }
    }

    @Test fun anUnresolvedMarkerSurvivesAnyNumberOfRestartsAndEditsUntouched() {
        val m = sent(text(), rec)
        var d = text().copy(saving = m)
        // Restart 1 (status unreadable): the marker moves to the unresolved list as it was.
        d = roundTrip(d)
        var f = form(ShareDraftRules.recover(d, listOf(unknown)))
        d = d.copy(saving = null, unresolved = f.keep)
        // The person edits, several times, with writes in between and restarts.
        for (title in listOf("a1", "a2", "a3 corrigido")) {
            d = roundTrip(d.copy(title = title))
            f = form(ShareDraftRules.recover(d, listOf(unknown)))
            d = d.copy(saving = null, unresolved = f.keep)
            assertEquals(listOf(m), d.unresolved)
        }
        // The marker is exactly what it was: operation, kind, target, fingerprint of what was SENT.
        assertEquals(ShareDraftRules.fingerprint(text(), rec), d.unresolved[0].fingerprint)
        assertEquals(id1, d.unresolved[0].operationId)
        // Much later the core answers "done": the form was edited, so nothing is deleted and the note is set.
        val last = form(ShareDraftRules.recover(d, listOf(done)))
        assertEquals(91L, last.earlier!!.documentId)
        assertTrue(last.keep.isEmpty())
    }

    // ------------------------------------------------------------------ the bug: edits after an unresolved save are never lost

    @Test fun editsMadeAfterAnUnresolvedSaveAreNotDeletedWhenTheOldOperationAnswersDone() {
        // 1: the save commits in the core, the reply is lost (death). 2: restart, status unreadable, form shown.
        val sentContent = text()
        val m = sent(sentContent, rec)
        val restored = roundTrip(sentContent.copy(saving = m))
        val f1 = form(ShareDraftRules.recover(restored, listOf(unknown)))
        // 3: the person edits and the draft is written (the old marker still the same).
        val edited = restored.copy(saving = null, unresolved = f1.keep, notes = "acrescentei isto")
        val onDisk = roundTrip(edited)
        // 4: later restart: the OLD operation answers done:true.
        val r = ShareDraftRules.recover(onDisk, listOf(done))
        assertTrue("o rascunho editado não pode ser apagado", r is ShareDraftRules.Recovery.Form)
        assertEquals("acrescentei isto", onDisk.notes)
        assertNotNull(form(r).earlier)
    }

    @Test fun anAttachMarkerKeptWhileTheTargetWasClearedIsNotCorruptAndNotDeleted() {
        val a = file()
        val m = sent(a, att)
        // The restored form had its changed target cleared, the marker kept: written with kind attach and target 0.
        val cleared = a.copy(mode = "CHOOSE", targetId = 0, targetNumber = "", targetTitle = "", saving = null, unresolved = listOf(m))
        val onDisk = ShareDraftRules.decode(ShareDraftRules.encode(cleared))
        assertTrue(onDisk is ShareDraftRules.Decoded.Ok)
        val back = (onDisk as ShareDraftRules.Decoded.Ok).draft
        assertEquals(listOf(m), back.unresolved)
        assertEquals(77L, back.unresolved[0].targetId)
        assertEquals("SEI 0412/2026", back.unresolved[0].targetNumber)
        // The same with the marker as the one "under way".
        val asSaving = (ShareDraftRules.decode(ShareDraftRules.encode(cleared.copy(saving = m, unresolved = emptyList()))) as ShareDraftRules.Decoded.Ok).draft
        assertEquals(m, asSaving.saving)
        // And on the disk it is a draft to resume, not one to drop.
        assertTrue(store.write(cleared.copy(size = 10, hash = "")))
        File(dir, "rascunho/in-1.bin").writeBytes(ByteArray(10))
        assertTrue(store.start(now, setUp = true) is ShareDraftStore.Start.Resume)
        // The core says done for that attach, but the form's content is not what was sent (no target now): kept.
        val r = form(ShareDraftRules.recover(back, listOf(ShareDraftRules.OperationStatus.Done(0))))
        assertEquals(77L, r.earlier!!.documentId)
    }

    // ------------------------------------------------------------------ saving again while a save is unresolved

    @Test fun savingTheSameContentAgainWhileUnknownSendsNoSecondCreate() {
        val d = text()
        val m = sent(d, rec)
        val g = ShareDraftRules.beforeSave(d.copy(unresolved = listOf(m)), rec, listOf(unknown))
        assertTrue(g is ShareDraftRules.SaveGate.Unconfirmed)
        assertEquals(listOf(m), (g as ShareDraftRules.SaveGate.Unconfirmed).keep)
        assertTrue(ShareDraftRules.MSG_CANNOT_CONFIRM_SAVE.contains("tente de novo", ignoreCase = true))
        // The status that could not even be asked is the same.
        assertTrue(ShareDraftRules.beforeSave(d.copy(unresolved = listOf(m)), rec, listOf(null)) is ShareDraftRules.SaveGate.Unconfirmed)
    }

    @Test fun savingTheSameContentAgainAfterTheCoreSaysNotDoneReusesTheId() {
        val d = text()
        val m = sent(d, rec)
        val g = ShareDraftRules.beforeSave(d.copy(unresolved = listOf(m)), rec, listOf(notDone)) as ShareDraftRules.SaveGate.Go
        assertTrue(g.keep.isEmpty())
        assertEquals(m, g.reuse)
        val p = ShareDraftRules.operationFor(
            ShareDraftRules.PendingOp(g.reuse!!.op, g.reuse!!.operationId, g.reuse!!.fingerprint), rec, ShareDraftRules.fingerprint(d, rec)
        ) { "NOVO" }
        assertEquals(id1, p.id)
    }

    @Test fun savingTheSameContentAgainAfterTheCoreSaysDoneIsAlreadySaved() {
        val d = text()
        val g = ShareDraftRules.beforeSave(d.copy(unresolved = listOf(sent(d, rec))), rec, listOf(done))
        assertEquals(91L, (g as ShareDraftRules.SaveGate.AlreadySaved).documentId)
    }

    @Test fun savingEditedContentWhileUnknownGoesOnUnderANewIdAndTheOldMarkerStaysBeside() {
        val old = text()
        val m1 = sent(old, rec, id1)
        val edited = text(title = "Ofício 0412 — versão 2")
        val gate = ShareDraftRules.beforeSave(edited.copy(unresolved = listOf(m1)), rec, listOf(unknown))
        val go = gate as ShareDraftRules.SaveGate.Go
        assertEquals(listOf(m1), go.keep)
        assertNull(go.reuse)
        // A NEW id (nothing pending for this content), and the marker of the new save is written next to the old one.
        val p = ShareDraftRules.operationFor(null, rec, ShareDraftRules.fingerprint(edited, rec)) { id2 }
        assertEquals(id2, p.id)
        val begun = ShareDraftRules.begin(edited.copy(saving = null, unresolved = go.keep), rec, p.id, now + 5)
        assertEquals(id2, begun.saving!!.operationId)
        assertEquals(listOf(m1), begun.unresolved)
        assertEquals(listOf(begun.saving, m1), begun.markers)
        // Both survive the disk, each bound to its own content.
        val back = roundTrip(begun)
        assertEquals(begun, back)
        assertEquals(ShareDraftRules.fingerprint(edited, rec), back.saving!!.fingerprint)
        assertEquals(ShareDraftRules.fingerprint(old, rec), back.unresolved[0].fingerprint)
    }

    @Test fun savingEditedContentWhenTheCoreSaysTheEarlierVersionWasSavedStopsAndTells() {
        val m1 = sent(text(), rec)
        val edited = text(title = "versão 2")
        val g = ShareDraftRules.beforeSave(edited.copy(unresolved = listOf(m1)), rec, listOf(done)) as ShareDraftRules.SaveGate.EarlierSaved
        assertEquals(91L, g.earlier.documentId)
        assertTrue(g.keep.isEmpty())
    }

    @Test fun twoUnresolvedSavesAreTrackedSeparatelyAndEachAnswersOnItsOwn() {
        val v1 = text()
        val v2 = text(title = "versão 2")
        val v3 = text(title = "versão 3")
        val m1 = sent(v1, rec, id1)
        val m2 = sent(v2, rec, id2)
        // The form now holds v3; the process died while v3 was being saved (marker m3), v1 and v2 unresolved.
        val m3 = sent(v3, rec, id3)
        val d = v3.copy(saving = m3, unresolved = listOf(m1, m2))
        assertEquals(listOf(m3, m2, m1), d.markers)
        // m3 unknown, m2 done (earlier content), m1 unknown: the edits stay, m1 and m3 stay tracked (oldest first).
        val f = form(ShareDraftRules.recover(d, listOf(unknown, done, unknown)))
        assertEquals(listOf(m1, m3), f.keep)
        assertEquals(91L, f.earlier!!.documentId)
        // m3 done with the form still holding v3: confirmed, the draft is over whatever the others say.
        assertEquals(91L, saved(ShareDraftRules.recover(d, listOf(done, unknown, notDone))).documentId)
        // The same operation is never listed twice when its id is reused.
        val again = ShareDraftRules.begin(d.copy(saving = null), rec, id2, now)
        assertEquals(listOf(m1), again.unresolved)
        assertEquals(id2, again.saving!!.operationId)
    }

    @Test fun theUnresolvedListIsCappedAndTheOldestIsForgotten() {
        val ids = (0 until 12).map { "%08d-0000-4000-8000-123456789abc".format(it) }
        val d = text().copy(unresolved = ids.map { DraftSaving(rec, it, now, fingerprint = "f$it") })
        val back = roundTrip(d)
        assertEquals(ShareDraftRules.MAX_UNRESOLVED, back.unresolved.size)
        assertEquals(ids.takeLast(ShareDraftRules.MAX_UNRESOLVED), back.unresolved.map { it.operationId })
        val begun = ShareDraftRules.begin(d.copy(unresolved = d.unresolved.takeLast(ShareDraftRules.MAX_UNRESOLVED)), rec, id1, now)
        assertEquals(ShareDraftRules.MAX_UNRESOLVED, begun.unresolved.size)
        assertTrue(ShareDraftRules.worstCaseBytes() < ShareDraftRules.MAX_SERIALIZED_BYTES)
    }

    // ------------------------------------------------------------------ the kind of the operation

    @Test fun aStatusOfAnotherKindIsUnknownNeverDone() {
        // The kind came in the answer and is not the marker's: unknown (also for "not done").
        assertEquals(unknown, ShareDraftRules.statusFrom(true, true, 5, kind = "attach", expectedKind = rec))
        assertEquals(unknown, ShareDraftRules.statusFrom(true, true, 5, kind = "record", expectedKind = att))
        assertEquals(unknown, ShareDraftRules.statusFrom(true, false, 0, kind = "attach", expectedKind = rec))
        // Same kind: as always.
        assertEquals(5L, (ShareDraftRules.statusFrom(true, true, 5, kind = "record", expectedKind = rec) as ShareDraftRules.OperationStatus.Done).documentId)
        assertEquals(notDone, ShareDraftRules.statusFrom(true, false, 0, kind = "record", expectedKind = rec))
        // No kind (an older core), or nothing to compare with: behaves as before.
        assertTrue(ShareDraftRules.statusFrom(true, true, 5) is ShareDraftRules.OperationStatus.Done)
        assertTrue(ShareDraftRules.statusFrom(true, true, 5, kind = null, expectedKind = rec) is ShareDraftRules.OperationStatus.Done)
        assertTrue(ShareDraftRules.statusFrom(true, true, 5, kind = "record", expectedKind = null) is ShareDraftRules.OperationStatus.Done)
    }

    @Test fun aDoneOfAnotherKindKeepsEverythingInTheRecovery() {
        val d = text()
        val m = sent(d, rec)
        val wrong = ShareDraftRules.OperationStatus.Done(91, kind = "attach")
        assertEquals(unknown, ShareDraftRules.effective(m, wrong))
        val f = form(ShareDraftRules.recover(d.copy(saving = m), listOf(wrong)))
        assertEquals(listOf(m), f.keep)
        assertTrue(f.unconfirmed)
        assertNull(f.earlier)
        // The same kind confirms; an absent kind behaves as today.
        assertEquals(91L, saved(ShareDraftRules.recover(d.copy(saving = m), listOf(ShareDraftRules.OperationStatus.Done(91, kind = "record")))).documentId)
        assertEquals(91L, saved(ShareDraftRules.recover(d.copy(saving = m), listOf(ShareDraftRules.OperationStatus.Done(91)))).documentId)
        // An attach marker against a "record" answer.
        val a = file()
        val am = sent(a, att)
        val f2 = form(ShareDraftRules.recover(a.copy(saving = am), listOf(ShareDraftRules.OperationStatus.Done(5, kind = "record"))))
        assertEquals(listOf(am), f2.keep)
    }

    // ------------------------------------------------------------------ the file: the marker is bound, and the old one still reads

    @Test fun aDraftFileWithAnUnresolvedMarkerRoundTripsWithEveryField() {
        val a = file(description = "descrição")
        val m1 = sent(text(), rec, id1)
        val m2 = sent(a, att, id2)
        val d = a.copy(saving = m2, unresolved = listOf(m1))
        val back = roundTrip(d)
        assertEquals(d, back)
        assertEquals(att, back.saving!!.op)
        assertEquals(77L, back.saving!!.targetId)
        assertEquals("SEI 0412/2026", back.saving!!.targetNumber)
        assertTrue(back.saving!!.bound)
        assertEquals(rec, back.unresolved[0].op)
        assertEquals(0L, back.unresolved[0].targetId)
        // On the disk it resumes with the check first, markers intact.
        assertTrue(store.write(d.copy(size = 10, hash = "")))
        File(dir, "rascunho/in-1.bin").writeBytes(ByteArray(10))
        val s = store.start(now, setUp = true) as ShareDraftStore.Start.Resume
        assertTrue(s.checkFirst)
        assertEquals(listOf(m2, m1), s.draft.markers)
    }

    @Test fun aFingerprintIsTheSameBeforeAndAfterTheDraftIsCutToWhatItMayHold() {
        val d = text().copy(notes = "n".repeat(300_000), title = "t".repeat(9_000))
        val fp = ShareDraftRules.fingerprint(d, rec)
        assertEquals(fp, ShareDraftRules.fingerprint(roundTrip(d), rec))
        assertEquals(91L, saved(ShareDraftRules.recover(roundTrip(d.copy(saving = sent(d, rec))), listOf(done))).documentId)
    }

    @Test fun theMarkerOfThePreviousBuildStillReadsAndIsNeverTakenForTheSameContent() {
        // What the build before this one wrote: sop/sat/sopid, no target, no fingerprint.
        val base = ShareDraftRules.encode(file().copy(copy = "in-1.bin"))
        val oldText = base.removeSuffix("}") + ",\"sop\":\"attach\",\"sat\":$now,\"sopid\":\"$id1\"}"
        val d = (ShareDraftRules.decode(oldText) as ShareDraftRules.Decoded.Ok).draft
        val m = d.saving!!
        assertTrue(m.usable)
        assertFalse(m.bound)
        assertEquals(att, m.op)
        assertEquals(77L, m.targetId)
        assertFalse(ShareDraftRules.sameContent(d, m))
        // Done: its edits could be anything, so nothing is deleted (the document it was aimed at is opened by the note).
        val f = form(ShareDraftRules.recover(d, listOf(ShareDraftRules.OperationStatus.Done(0))))
        assertEquals(77L, f.earlier!!.documentId)
        // Not done: dropped. Unknown: kept.
        assertTrue(form(ShareDraftRules.recover(d, listOf(notDone))).keep.isEmpty())
        assertNull(form(ShareDraftRules.recover(d, listOf(notDone))).reuse)
        assertEquals(listOf(m), form(ShareDraftRules.recover(d, listOf(unknown))).keep)
        // The old attach marker on a form whose target is empty is not corrupt either (a record's the same).
        val noTarget = base.replace("\"tid\":77", "\"tid\":0").removeSuffix("}") + ",\"sop\":\"attach\",\"sat\":$now,\"sopid\":\"$id1\"}"
        assertTrue(ShareDraftRules.decode(noTarget) is ShareDraftRules.Decoded.Ok)
        // A marker is "the same content" only when its stored fingerprint matches the current content.
        val bound = sent(d, att)
        assertTrue(ShareDraftRules.sameContent(d, bound))
        assertFalse(ShareDraftRules.sameContent(d.copy(description = "x"), bound))
        assertFalse(ShareDraftRules.sameContent(d, bound.copy(fingerprint = "")))
    }

    @Test fun aDamagedMarkerInTheUnresolvedListMakesTheFileCorruptButAFormTargetNeverDoes() {
        val good = ShareDraftRules.encode(text().copy(unresolved = listOf(sent(text(), rec, id1))))
        assertTrue(ShareDraftRules.decode(good) is ShareDraftRules.Decoded.Ok)
        assertTrue(ShareDraftRules.decode(good.replace("\"u0op\":\"record\"", "\"u0op\":\"voar\"")) is ShareDraftRules.Decoded.Corrupt)
        assertTrue(ShareDraftRules.decode(good.replace("\"u0tid\":0", "\"u0tid\":\"x\"")) is ShareDraftRules.Decoded.Corrupt)
        assertTrue(ShareDraftRules.decode(good.replace("\"u0tid\":0", "\"u0tid\":-3")) is ShareDraftRules.Decoded.Corrupt)
    }
}
