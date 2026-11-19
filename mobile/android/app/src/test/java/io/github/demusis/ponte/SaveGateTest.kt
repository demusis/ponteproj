package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SaveGateTest {
    @Test fun secondTapWhileSavingStartsNothing() {
        val first = SaveGate.begin(SaveState())
        assertEquals(SaveState(busy = true), first)
        assertNull(SaveGate.begin(first!!))
    }

    @Test fun refusalKeepsReasonAndAllowsRetry() {
        val failed = SaveGate.failed("sem espaço")
        assertFalse(failed.busy)
        assertEquals("sem espaço", failed.error)
        val retry = SaveGate.begin(failed)
        assertEquals(SaveState(busy = true), retry) // the reason is cleared by the new attempt
    }

    @Test fun confirmationClearsEverything() {
        assertEquals(SaveState(), SaveGate.done())
    }

    @Test fun resultClosesOnlyTheScreenThatStartedIt() {
        val stack = mutableListOf<Route>(Route.NewTask(3), Route.Note(3))
        assertTrue(SaveGate.close(stack, Route.Note(3)))
        assertEquals(listOf<Route>(Route.NewTask(3)), stack)
        assertFalse(SaveGate.close(stack, Route.Note(3))) // already gone: nothing else is closed
        assertEquals(listOf<Route>(Route.NewTask(3)), stack)
    }
}
