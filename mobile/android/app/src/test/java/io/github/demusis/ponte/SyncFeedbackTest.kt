package io.github.demusis.ponte

import org.junit.Assert.*
import org.junit.Test

class SyncFeedbackTest {
    @Test fun refusalsAreVisibleOnlyForUserRequests() {
        for (reason in SyncRefusal.entries) {
            val user = SyncFeedback.decide(reason, auto = false)
            val automatic = SyncFeedback.decide(reason, auto = true)
            assertEquals(reason.name, user.status, user.message)
            assertNull(reason.name, automatic.message)
            assertEquals(reason.name, user.status, automatic.status)
            assertFalse(reason.name, automatic.mobileAction)
            assertEquals(reason.name, reason == SyncRefusal.NO_WIFI, user.mobileAction)
        }
    }

    @Test fun cancellationAndBusyExplainWhatHappened() {
        assertEquals("Sincronização cancelada: a senha não foi liberada.", SyncFeedback.decide(SyncRefusal.PASSPHRASE_CANCELLED, false).message)
        assertEquals("Já há uma sincronização em andamento.", SyncFeedback.decide(SyncRefusal.BUSY, false).message)
    }
}
