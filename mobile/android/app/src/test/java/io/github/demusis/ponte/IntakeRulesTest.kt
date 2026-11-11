package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZonedDateTime

/** The decisions of Intake (what to do with a share, with an attach result, with the notice), as pure functions. */
class IntakeRulesTest {
    private val R = IntakeRules.Result.values()

    @Test fun anAttachResultIsParkedWhileLockedAndAttachedOtherwise() {
        assertEquals(IntakeRules.Result.ATTACH, IntakeRules.attachResult(ok = true, usable = true, locked = false))
        assertEquals(IntakeRules.Result.PARK, IntakeRules.attachResult(ok = true, usable = true, locked = true))
        // Nothing usable is dropped, locked or not: the person gave up, or the file is empty or gone.
        for (locked in listOf(false, true)) {
            assertEquals(IntakeRules.Result.DROP, IntakeRules.attachResult(ok = false, usable = true, locked = locked))
            assertEquals(IntakeRules.Result.DROP, IntakeRules.attachResult(ok = true, usable = false, locked = locked))
        }
        assertEquals(3, R.size)
    }

    @Test fun anUnseenShareIsDroppedWhenTheActivityStopsLocked() {
        fun drop(
            changing: Boolean = false, locked: Boolean = true, unlocking: Boolean = false,
            busy: Boolean = false, seen: Boolean = false, has: Boolean = true
        ) = IntakeRules.dropShareOnStop(changing, locked, unlocking, busy, seen, has)
        assertTrue(drop())
        // The unlock prompt stops the Activity too: not a reason to drop.
        assertFalse(drop(unlocking = true))
        assertFalse(drop(changing = true))
        assertFalse(drop(locked = false))
        assertFalse(drop(busy = true))
        // Once shown, it waits behind later locks like a form in progress.
        assertFalse(drop(seen = true))
        assertFalse(drop(has = false))
    }

    @Test fun aParkedResultIsDroppedOnlyWhenLeavingLockedWithoutUnlocking() {
        assertTrue(IntakeRules.dropParkedOnStop(false, true, false, true))
        assertFalse(IntakeRules.dropParkedOnStop(false, true, true, true))
        assertFalse(IntakeRules.dropParkedOnStop(true, true, false, true))
        assertFalse(IntakeRules.dropParkedOnStop(false, false, false, true))
        assertFalse(IntakeRules.dropParkedOnStop(false, true, false, false))
    }

    @Test fun lateWorkIsRecognisedByTheGeneration() {
        assertFalse(IntakeRules.isLate(3, 3))
        assertTrue(IntakeRules.isLate(4, 3))
    }

    @Test fun aSecondShareKeepsTheFirst() {
        assertEquals(IntakeRules.Arrival.ACCEPT, IntakeRules.arrival(reading = false, hasItem = false))
        assertEquals(IntakeRules.Arrival.KEEP_FIRST, IntakeRules.arrival(reading = true, hasItem = false))
        assertEquals(IntakeRules.Arrival.KEEP_FIRST, IntakeRules.arrival(reading = false, hasItem = true))
        assertEquals(
            "Termine ou cancele o que está guardando antes de receber outro arquivo.", ShareRules.MSG_KEEP_FIRST
        )
    }

    @Test fun aCancelledCopyStopsEarlyAndWritesNothingMore() {
        val out = ByteArrayOutputStream()
        var calls = 0
        try {
            BoundedCopy.copy(ByteArrayInputStream(ByteArray(300_000)), out, 1_000_000) { calls++ >= 1 }
            fail("should stop")
        } catch (_: CopyCancelled) {
        }
        assertEquals(64 * 1024, out.size())
        // Cancelled before anything: nothing is written.
        val none = ByteArrayOutputStream()
        try {
            BoundedCopy.copy(ByteArrayInputStream(ByteArray(10)), none, 100) { true }
            fail("should stop")
        } catch (_: CopyCancelled) {
        }
        assertEquals(0, none.size())
    }

    @Test fun workFileNamesAreUnique() {
        val dir = File(System.getProperty("java.io.tmpdir"), "ponte-intake-test-" + System.nanoTime()).apply { mkdirs() }
        try {
            val names = (1..300).map { File.createTempFile("in-", ".tmp", dir).name }
            assertEquals(300, names.toSet().size)
        } finally {
            dir.listFiles()?.forEach { it.delete() }
            dir.delete()
        }
    }

    // ------------------------------------------------------------------ images

    @Test fun anImageIsReencodedWhenItsRealTypeIsNotAccepted() {
        val limit = 8L * 1024 * 1024
        for (ok in listOf("image/jpeg", "image/png", "image/gif", "image/bmp", "IMAGE/PNG")) {
            assertFalse(ok, ShareRules.needsReencode(ok, 1000, limit))
        }
        for (bad in listOf("image/webp", "image/heic", "image/heif", "image/avif")) {
            assertTrue(bad, ShareRules.needsReencode(bad, 1000, limit))
        }
        // Over the limit, whatever it is; exactly the limit stays.
        assertTrue(ShareRules.needsReencode("image/jpeg", limit + 1, limit))
        assertFalse(ShareRules.needsReencode("image/jpeg", limit, limit))
        // A type the decoder cannot read is left to the core.
        assertFalse(ShareRules.needsReencode(null, 1000, limit))
        assertTrue(ShareRules.needsReencode(null, limit + 1, limit))
    }

    @Test fun theFormStartsWithOutroOrTheFirstKind() {
        assertEquals("outro", ShareRules.defaultKind(listOf("memorando", "outro", "papel")))
        assertEquals("papel", ShareRules.defaultKind(listOf("papel")))
        assertEquals("outro", ShareRules.defaultKind(emptyList()))
    }

    // ------------------------------------------------------------------ the launcher's guard

    @Test fun onlyReadOnlyScreensMayBeLeftForHoje() {
        assertFalse(NavGuard.busy(emptyList(), false))
        assertFalse(NavGuard.busy(listOf(Route.Document(1), Route.Text(1), Route.Attendance(2)), false))
        val unsaved = listOf(
            Route.Photo, Route.Share, Route.SharePick, Route.Settings, Route.Setup, Route.Search,
            Route.Account(""), Route.Account("u1"), Route.Note(1), Route.NewTask(0), Route.EditTask(3), Route.Pick(3)
        )
        for (r in unsaved) assertTrue(r.toString(), NavGuard.busy(listOf(Route.Document(1), r), false))
        // An attach in progress counts, even from a plain ficha.
        assertTrue(NavGuard.busy(listOf(Route.Document(1)), true))
    }

    // ------------------------------------------------------------------ the notice and the library

    @Test fun theNoticeNeverCreatesAKeyOrALibrary() {
        // Open already: reads, whatever the files say.
        assertTrue(IntakeRules.noticeMayRead(ready = true, keyBroken = false, libraryFileExists = false, keyStored = false))
        // Cold: only with both the library file and a key stored before.
        assertTrue(IntakeRules.noticeMayRead(false, false, libraryFileExists = true, keyStored = true))
        assertFalse(IntakeRules.noticeMayRead(false, false, libraryFileExists = false, keyStored = true))
        assertFalse(IntakeRules.noticeMayRead(false, false, libraryFileExists = true, keyStored = false))
        assertFalse(IntakeRules.noticeMayRead(false, false, false, false))
        // A key that cannot be used: nothing is read.
        assertFalse(IntakeRules.noticeMayRead(true, keyBroken = true, libraryFileExists = true, keyStored = true))
    }

    // ------------------------------------------------------------------ when the notice rings next

    private val zone = ZoneId.of("America/Sao_Paulo")
    private val eight = DeadlineNotice.Time(8, 0)
    private fun at(day: Int, h: Int, m: Int = 0) = ZonedDateTime.of(2026, 11, day, h, m, 0, 0, zone)
    private fun day(d: Int) = LocalDate.of(2026, 11, d)

    @Test fun beforeTheHourItRingsAtTheHour() {
        assertEquals(at(10, 8), DeadlineNotice.nextAlarm(at(10, 6, 30), eight, shownOn = day(9)))
        assertEquals(at(10, 8), DeadlineNotice.nextAlarm(at(10, 6, 30), eight, shownOn = null))
    }

    @Test fun afterTheHourNotShownYetItRingsSoonNotTomorrow() {
        val now = at(10, 9, 15)
        assertEquals(now.plusSeconds(DeadlineNotice.SOON_SECONDS), DeadlineNotice.nextAlarm(now, eight, shownOn = day(9)))
        assertEquals(now.plusSeconds(DeadlineNotice.SOON_SECONDS), DeadlineNotice.nextAlarm(now, eight, shownOn = null))
        // Exactly at the hour counts as after it.
        assertEquals(at(10, 8).plusSeconds(60), DeadlineNotice.nextAlarm(at(10, 8), eight, shownOn = null))
    }

    @Test fun afterTheHourAlreadyShownItRingsTomorrow() {
        assertEquals(at(11, 8), DeadlineNotice.nextAlarm(at(10, 9, 15), eight, shownOn = day(10)))
        // Shown today, and the clock is still before the hour (the time was changed): still tomorrow.
        assertEquals(at(11, 8), DeadlineNotice.nextAlarm(at(10, 7), eight, shownOn = day(10)))
    }

    @Test fun aNewDayStartsOver() {
        // Shown yesterday: before the hour today it rings at the hour, after it soon.
        assertEquals(at(11, 8), DeadlineNotice.nextAlarm(at(11, 5), eight, shownOn = day(10)))
        assertEquals(at(11, 8, 30).plusSeconds(60), DeadlineNotice.nextAlarm(at(11, 8, 30), eight, shownOn = day(10)))
    }

    @Test fun aClockSetBackCountsAsNotShown() {
        // The saved day is in the future (the clock went back): not today, so it behaves as never shown.
        assertEquals(at(10, 8), DeadlineNotice.nextAlarm(at(10, 6), eight, shownOn = day(15)))
        val now = at(10, 12)
        assertEquals(now.plusSeconds(60), DeadlineNotice.nextAlarm(now, eight, shownOn = day(15)))
    }

    @Test fun theTextsAreAsAsked() {
        assertEquals("Ponte de bolso: há prazos a conferir.", DeadlineNotice.PUBLIC_TEXT)
        assertEquals("Configure o Ponte de bolso antes de guardar algo nele.", ShareRules.MSG_NOT_SET_UP)
        assertEquals("Até 8 MB. Aceita PDF, documentos do Office, texto, imagens e e-mails.", AttachLimits.upFront())
    }
}
