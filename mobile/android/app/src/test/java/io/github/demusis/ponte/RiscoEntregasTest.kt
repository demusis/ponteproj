package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Test

class RiscoEntregasTest {
    private fun item(kind: String, end: String = "") =
        RiskItem(1, "Entrega", "2026-11-25", kind, end, 2, "Reunir documentos")

    @Test fun latePlanned() =
        assertEquals("fim planejado 26/11 passa do prazo 25/11", RiskRules.riskLine(item("late_planned", "2026-11-26T18:00")))

    @Test fun lateProjected() = assertEquals(
        "previsto até 26/11, depois do prazo 25/11 · por causa de Reunir documentos",
        RiskRules.riskLine(item("late_projected", "2026-11-26T18:00"))
    )

    @Test fun missingPlan() = assertEquals("sem previsão: falta planejar Reunir documentos", RiskRules.riskLine(item("missing_plan")))

    @Test fun missingEstimate() = assertEquals("sem previsão: falta estimar Reunir documentos", RiskRules.riskLine(item("missing_estimate")))

    @Test fun dateFormatting() {
        assertEquals("05/01", RiskRules.shortDate("2026-01-05"))
        assertEquals("26/11", RiskRules.shortDate("2026-11-26T18:00"))
        assertEquals("xx", RiskRules.shortDate("xx"))
    }

    @Test fun tapOpensTheCause() {
        assertEquals(2L, RiskRules.openTarget(item("missing_plan")))
        assertEquals(1L, RiskRules.openTarget(item("late_planned").copy(causeId = 0)))
    }
}
