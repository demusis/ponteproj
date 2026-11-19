package io.github.demusis.ponte

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HolidaysTest {
    private fun d(y: Int, m: Int, day: Int) = LocalDate.of(y, m, day)

    @Test fun pascoa() {
        assertEquals(d(2024, 3, 31), Holidays.easter(2024))
        assertEquals(d(2025, 4, 20), Holidays.easter(2025))
        assertEquals(d(2026, 4, 5), Holidays.easter(2026))
        assertEquals(d(2027, 3, 28), Holidays.easter(2027))
    }

    @Test fun feriadosMoveis2026() {
        assertEquals("Carnaval", Holidays.holiday(d(2026, 2, 16)))
        assertEquals("Carnaval", Holidays.holiday(d(2026, 2, 17)))
        assertNull(Holidays.holiday(d(2026, 2, 18)))
        assertEquals("Sexta-feira Santa", Holidays.holiday(d(2026, 4, 3)))
        assertEquals("Corpus Christi", Holidays.holiday(d(2026, 6, 4)))
    }

    @Test fun feriadosFixosEConscienciaNegra() {
        assertEquals("Tiradentes", Holidays.holiday(d(2026, 4, 21)))
        assertEquals("Natal", Holidays.holiday(d(2026, 12, 25)))
        assertNull(Holidays.holiday(d(2023, 11, 20)))
        assertEquals("Dia da Consciência Negra", Holidays.holiday(d(2026, 11, 20)))
    }

    @Test fun diasUteis() {
        assertTrue(Holidays.isWorkingDay(d(2026, 4, 22)))
        assertFalse(Holidays.isWorkingDay(d(2026, 4, 21)))
        assertFalse(Holidays.isWorkingDay(d(2026, 4, 25)))
        assertFalse(Holidays.isWorkingDay(d(2026, 11, 20)))
    }
}
