package io.github.demusis.ponte

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CalendarScheduleTest {
    @Test fun requiresSwitchAndPermission() {
        assertTrue(CalendarSchedule.shouldSchedule(true, true))
        assertFalse(CalendarSchedule.shouldSchedule(false, true))
        assertFalse(CalendarSchedule.shouldSchedule(true, false))
        assertFalse(CalendarSchedule.shouldSchedule(false, false))
    }
}
