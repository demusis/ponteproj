package io.github.demusis.ponte

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The rules of "Cobranças de hoje" that need no Android: what is sent to the core and what is said. */
class CobrancasRegrasTest {
    private val today = LocalDate.of(2026, 11, 19)

    @Test fun contactTextIsTheWayChosenAndTheNote() {
        assertEquals("Cobrado por e-mail", FollowUpRules.contactText("Cobrado por e-mail", ""))
        assertEquals("Cobrado por e-mail: sem resposta", FollowUpRules.contactText(" Cobrado por e-mail ", "  sem resposta  "))
    }

    @Test fun withoutAWayTheContactIsTheComputersDefault() {
        assertEquals("Cobrado por telefone", FollowUpRules.DEFAULT_CHANNEL)
        assertEquals("Cobrado por telefone", FollowUpRules.contactText("", ""))
        assertEquals("Cobrado por telefone: ligar de novo", FollowUpRules.contactText("  ", "ligar de novo"))
    }

    @Test fun theNextDateMustBeARealDateAfterToday() {
        assertTrue(FollowUpRules.nextDateOk("2026-11-20", today))
        assertTrue(FollowUpRules.nextDateOk(" 2027-01-02 ", today))
        assertFalse("today is not ahead", FollowUpRules.nextDateOk("2026-11-19", today))
        assertFalse(FollowUpRules.nextDateOk("2026-11-18", today))
        assertFalse(FollowUpRules.nextDateOk("", today))
        assertFalse(FollowUpRules.nextDateOk("amanhã", today))
        assertFalse(FollowUpRules.nextDateOk("2026-02-30", today))
    }

    @Test fun theQuickChoicesAreAfterTodayAndStartsAtThreeDays() {
        assertEquals(listOf("Amanhã", "Em 3 dias", "Em 1 semana"), FollowUpRules.PRESETS.map { it.first })
        assertEquals(listOf("2026-11-20", "2026-11-22", "2026-11-26"), FollowUpRules.PRESETS.map { FollowUpRules.dateIn(today, it.second) })
        assertTrue(FollowUpRules.PRESETS.all { FollowUpRules.nextDateOk(FollowUpRules.dateIn(today, it.second), today) })
        assertEquals("2026-11-22", FollowUpRules.dateIn(today, FollowUpRules.DEFAULT_NEXT_DAYS))
        // The month and the year turn over.
        assertEquals("2027-01-02", FollowUpRules.dateIn(LocalDate.of(2026, 12, 30), 3))
    }

    @Test fun theChipSaysTodayOrHowLateItIs() {
        assertEquals("Hoje", FollowUpRules.chip(0))
        assertEquals("Atrasada 1 dia", FollowUpRules.chip(1))
        assertEquals("Atrasada 5 dias", FollowUpRules.chip(5))
        assertEquals("Cobranças de hoje · 3", FollowUpRules.title(3))
    }

    @Test fun theMessagesNameTheDate() {
        assertEquals("Cobrança registrada.", FollowUpRules.registeredMessage(""))
        assertEquals("Cobrança registrada. Próxima em 22/11/2026.", FollowUpRules.registeredMessage("2026-11-22"))
        assertEquals("Próxima cobrança marcada para 26/11/2026.", FollowUpRules.scheduledMessage("2026-11-26"))
    }

    @Test fun anAtendimentoRowIsNotAFollowUpUnlessTheCoreSaidWhy() {
        val row = AttendanceItem(1, "ORG-1", "Assunto", "Em análise", "2026-12-01", "", false)
        assertEquals("", row.followUp)
        assertEquals(0, row.followUpDays)
        val chase = row.copy(followUp = "Cobrança marcada para hoje")
        assertEquals("Cobrança marcada para hoje", chase.followUp)
        // The search of the Atendimentos list filters the cobranças too.
        assertTrue(AttendanceRules.matches(chase, "org-1"))
        assertFalse(AttendanceRules.matches(chase, "outro"))
    }
}
