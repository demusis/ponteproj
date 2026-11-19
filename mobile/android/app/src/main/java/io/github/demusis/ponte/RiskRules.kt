package io.github.demusis.ponte

/** One delivery at risk, as the core reports it (see PonteCore.atRisk). */
internal data class RiskItem(
    val taskId: Long,
    val title: String,
    val dueDate: String,
    /** late_planned | late_projected | missing_plan | missing_estimate */
    val kind: String,
    /** The projected end ("AAAA-MM-DDTHH:MM"), or "" when there is none. */
    val end: String,
    val causeId: Long,
    val causeTitle: String
)

internal object RiskRules {
    val KINDS = setOf("late_planned", "late_projected", "missing_plan", "missing_estimate")

    /** "2026-11-26" or "2026-11-26T18:00" → "26/11"; anything else is kept. */
    fun shortDate(value: String): String {
        val d = try { java.time.LocalDate.parse(value.trim().take(10)) } catch (_: Throwable) { return value.trim() }
        return "%02d/%02d".format(d.dayOfMonth, d.monthValue)
    }

    /** The muted line under the title of a risk row. */
    fun riskLine(item: RiskItem): String {
        val due = shortDate(item.dueDate)
        val end = shortDate(item.end)
        return when (item.kind) {
            "late_planned" -> "fim planejado $end passa do prazo $due"
            "late_projected" -> "previsto até $end, depois do prazo $due · por causa de ${item.causeTitle}"
            "missing_plan" -> "sem previsão: falta planejar ${item.causeTitle}"
            "missing_estimate" -> "sem previsão: falta estimar ${item.causeTitle}"
            else -> ""
        }
    }

    /** The task a tap opens: the cause (the task that must be planned or estimated), else the delivery itself. */
    fun openTarget(item: RiskItem): Long = if (item.causeId > 0) item.causeId else item.taskId
}
