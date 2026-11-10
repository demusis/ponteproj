package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The lock decisions are plain Kotlin: no phone and no pictures needed. */
class LockPolicyTest {
    private val min = 60_000L

    private fun lock(
        enabled: Boolean = true, secure: Boolean = true, delay: LockDelay = LockDelay.ONE_MINUTE,
        away: Long? = null, cold: Boolean = false
    ) = LockPolicy.shouldLock(enabled, secure, delay, away, cold)

    @Test fun coldStartLocks() = assertTrue(lock(cold = true))

    @Test fun coldStartDoesNotLockWhenSwitchedOff() = assertFalse(lock(enabled = false, cold = true))

    @Test fun neverLocksWithoutScreenLock() {
        assertFalse(lock(secure = false, cold = true))
        assertFalse(lock(secure = false, delay = LockDelay.NOW, away = 10 * min))
    }

    @Test fun notAwayDoesNotLock() = assertFalse(lock(delay = LockDelay.NOW, away = null))

    @Test fun immediatelyLocksAfterAnyTimeAway() {
        assertTrue(lock(delay = LockDelay.NOW, away = 0))
        assertTrue(lock(delay = LockDelay.NOW, away = 1))
    }

    @Test fun oneMinute() {
        assertFalse(lock(away = 59_000))
        assertFalse(lock(away = min))
        assertTrue(lock(away = min + 1))
    }

    @Test fun fiveMinutes() {
        assertFalse(lock(delay = LockDelay.FIVE_MINUTES, away = 4 * min))
        assertFalse(lock(delay = LockDelay.FIVE_MINUTES, away = 5 * min))
        assertTrue(lock(delay = LockDelay.FIVE_MINUTES, away = 5 * min + 1))
    }

    @Test fun clockThatRestartedCountsAsLong() = assertTrue(lock(delay = LockDelay.FIVE_MINUTES, away = -5))

    @Test fun switchedOffNeverLocksAfterAway() = assertFalse(lock(enabled = false, delay = LockDelay.NOW, away = 99 * min))

    @Test fun defaultIsOnOnlyWithScreenLock() {
        assertTrue(LockPolicy.defaultEnabled(true))
        assertFalse(LockPolicy.defaultEnabled(false))
    }

    @Test fun activeNeedsBoth() {
        assertTrue(LockPolicy.active(true, true))
        assertFalse(LockPolicy.active(true, false))
        assertFalse(LockPolicy.active(false, true))
    }

    @Test fun delaysFromKeys() {
        assertEquals(LockDelay.NOW, LockDelay.fromKey("agora"))
        assertEquals(LockDelay.ONE_MINUTE, LockDelay.fromKey("1"))
        assertEquals(LockDelay.FIVE_MINUTES, LockDelay.fromKey("5"))
        assertEquals(LockDelay.ONE_MINUTE, LockDelay.fromKey(null))
        assertEquals(LockDelay.ONE_MINUTE, LockDelay.fromKey("lixo"))
    }

    @Test fun unlockReleasesPassphraseOnlyWhenOnePromptCanDoBoth() {
        assertTrue(LockPolicy.unlockReleasesPassphrase(34, stored = true, inMemory = false, syncDue = true))
        assertFalse(LockPolicy.unlockReleasesPassphrase(29, stored = true, inMemory = false, syncDue = true))
        assertFalse(LockPolicy.unlockReleasesPassphrase(34, stored = false, inMemory = false, syncDue = true))
        assertFalse(LockPolicy.unlockReleasesPassphrase(34, stored = true, inMemory = true, syncDue = true))
        assertFalse(LockPolicy.unlockReleasesPassphrase(34, stored = true, inMemory = false, syncDue = false))
    }

    @Test fun photosInProgressAreKept() {
        val keep = setOf("/c/fotos/foto-1.jpg")
        assertFalse(LockPolicy.mayDeletePhoto("/c/fotos/foto-1.jpg", keep, saving = false))
        assertTrue(LockPolicy.mayDeletePhoto("/c/fotos/foto-0.jpg", keep, saving = false))
    }

    @Test fun nothingInPhotosIsDeletedWhileSaving() =
        assertFalse(LockPolicy.mayDeletePhoto("/c/fotos/envio-2.jpg", emptySet(), saving = true))

    @Test fun failedKeyPromptIsNeverUsedAgain() =
        assertFalse(LockPolicy.unlockReleasesPassphrase(34, true, false, true, failedBefore = true))

    @Test fun promptPendingWhenLeaving() {
        // success: fresh authentication, never locks
        assertFalse(LockPolicy.lockAfterPrompt(true, true, LockDelay.NOW, 10 * min, promptSucceeded = true))
        // failure or cancel: decided by the time away
        assertTrue(LockPolicy.lockAfterPrompt(true, true, LockDelay.NOW, 5, promptSucceeded = false))
        assertTrue(LockPolicy.lockAfterPrompt(true, true, LockDelay.ONE_MINUTE, 2 * min, promptSucceeded = false))
        assertFalse(LockPolicy.lockAfterPrompt(true, true, LockDelay.ONE_MINUTE, 10_000, promptSucceeded = false))
        assertFalse(LockPolicy.lockAfterPrompt(false, true, LockDelay.NOW, 2 * min, promptSucceeded = false))
    }
}
