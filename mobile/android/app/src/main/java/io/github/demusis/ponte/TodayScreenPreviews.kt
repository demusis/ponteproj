package io.github.demusis.ponte

import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.Preview
import java.time.LocalDate

private fun previewTask(id: Long, title: String) = TaskItem(
    id = id, documentId = 0, title = title, actionLabel = "", documentNumber = "",
    dueDate = "", dueChip = "há 2 dias", done = false, overdue = true, dueToday = false
)

private fun previewWeek(entries: List<WeekEntry>) =
    WeekPlan.days(WeekPlan.mondayOf(LocalDate.now()), entries)

@Composable
private fun PreviewHost(state: TodayState, week: List<WeekDay>, risks: List<RiskItem> = emptyList()) {
    PonteTheme {
        TodayScreen(state, onToggle = {}, onOpen = {}, week = week, weekStart = WeekPlan.mondayOf(LocalDate.now()), risks = risks, followUpCount = 2)
    }
}

/** Empty day, nothing due. */
@Preview(widthDp = 360, heightDp = 800, showBackground = true)
@Composable
private fun TodayEmptyPreview() {
    PreviewHost(TodayState(loading = false), previewWeek(emptyList()))
}

/** A busy day: a long title, a resource use, a dependency warning, a deadline and a late task. */
@Preview(widthDp = 360, heightDp = 800, showBackground = true)
@Composable
private fun TodayBusyPreview() {
    val d = LocalDate.now().toString()
    val entries = WeekPlan.task(
        "a", "Elaborar a minuta da resposta ao ofício sobre a perícia contábil do processo principal",
        "${d}T09:00", "${d}T10:30", 0, d
    ).map { it.copy(dependencyNotice = "⚠ começa antes de terminar Reunir documentos", dependencyWarning = true) } +
        WeekPlan.use(
            ResourceUseItem(
                uid = "u", resourceUid = "r", resourceName = "Sala de reuniões 2", quantity = 1, who = "", sector = "",
                purpose = "", start = "${d}T14:00", end = "${d}T16:00", returnedAt = "", status = "reservado",
                statusLabel = "Reservado", notes = "", overdue = false, period = ""
            )
        )!!
    val state = TodayState(
        loading = false, late = listOf(previewTask(1, "Responder o ofício")),
        summary = "1 atrasada · 1 para hoje"
    )
    val risks = listOf(
        RiskItem(1, "Entregar o laudo pericial", "2026-11-25", "late_projected", "2026-11-26T18:00", 2, "Reunir documentos"),
        RiskItem(3, "Responder o ofício", "2026-11-30", "missing_estimate", "", 4, "Levantar os valores")
    )
    PreviewHost(state, previewWeek(entries), risks)
}
