package io.github.demusis.ponte

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * Form of a new task. [onCreate] gets the title, the action key, the due date (ISO or empty) and the
 * responsible person, which starts as the person's own name ([myName]) when it is set and can be
 * changed or cleared before saving.
 */
@Composable
internal fun NewTaskScreen(
    document: String,
    actions: List<Choice>,
    onBack: () -> Unit,
    onCreate: (String, String, String, String, List<String>, NewPlanning, String) -> Unit,
    withoutDocument: Boolean = false,
    assignees: List<String> = emptyList(),
    myName: String = "",
    onCreateCategory: (String, Int, (Category?, String) -> Unit) -> Unit = { _, _, done -> done(null, "") },
    remindersBlocked: Boolean = false,
    onRemindersChosen: () -> Unit = {},
    saving: Boolean = false,
    saveError: String = "",
    projects: List<ProjectItem> = emptyList(),
    /** The project the task starts in (empty: none). */
    projectUid: String = ""
) {
    var title by rememberSaveable { mutableStateOf("") }
    var action by rememberSaveable { mutableStateOf(actions.firstOrNull()?.key ?: "") }
    var due by rememberSaveable { mutableStateOf("") }
    var assignee by rememberSaveable { mutableStateOf(myName) }
    var categories by rememberSaveable { mutableStateOf(emptyList<String>()) }
    var project by rememberSaveable { mutableStateOf(projectUid) }
    var tried by rememberSaveable { mutableStateOf(false) }
    var start by rememberSaveable { mutableStateOf("") }
    var end by rememberSaveable { mutableStateOf("") }
    var startHour by rememberSaveable { mutableStateOf("") }
    var endHour by rememberSaveable { mutableStateOf("") }
    var daysText by rememberSaveable { mutableStateOf("") }
    var estimate by rememberSaveable { mutableIntStateOf(0) }
    var reminders by rememberSaveable { mutableStateOf(emptyList<Int>()) }
    val planning = NewPlanning(start, end, estimate, reminders)
    val hourInvalid = planningHourInvalid(start, end, startHour, endHour)
    val planningProblem = planning.problem()

    DetailScaffold(
        title = "Nova tarefa", onBack = { if (!saving) onBack() }, close = true,
        footer = {
            if (saveError.isNotEmpty()) {
                Text(
                    saveError, Modifier.padding(bottom = 8.dp),
                    style = MaterialTheme.typography.bodySmall, color = Ponte.colors.error
                )
            }
            if (planningProblem.isNotEmpty()) {
                Text(
                    planningProblem, Modifier.padding(bottom = 8.dp),
                    style = MaterialTheme.typography.bodySmall, color = Ponte.colors.error
                )
            }
            PrimaryButton("Criar tarefa", enabled = !hourInvalid && planningProblem.isEmpty(), loading = saving, onClick = {
                if (title.isBlank()) tried = true else onCreate(title.trim(), action, due, assignee.trim(), categories, planning, project)
            })
        }
    ) {
        Column(
            Modifier.fillMaxSize().verticalScroll(rememberScrollState())
                .padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp)
        ) {
            if (document.isNotBlank()) {
                Text(
                    "No documento $document", Modifier,
                    style = MaterialTheme.typography.bodyMedium, color = Ponte.colors.muted,
                    maxLines = 2, overflow = TextOverflow.Ellipsis
                )
            } else if (withoutDocument) {
                Text(
                    "Tarefa sem documento. Depois você pode vinculá-la a um documento.", Modifier,
                    style = MaterialTheme.typography.bodyMedium, color = Ponte.colors.muted
                )
            }
            LabeledField(
                "Título", title, { title = it },
                placeholder = "O que precisa ser feito",
                error = if (tried && title.isBlank()) "Informe o título." else "",
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences)
            )
            if (actions.isNotEmpty()) ChoiceField("Ação", actions, action, { action = it })
            DateField("Prazo (opcional)", due, { due = it }, emptyText = "Sem prazo")
            PlanningFields(
                due, start, end, estimate, startHour, endHour, daysText,
                onStartHour = { startHour = it }, onEndHour = { endHour = it }, onDaysText = { daysText = it },
                onChange = { s, e, m -> start = s; end = e; estimate = m }
            )
            ReminderChips(reminders, { reminders = it }, remindersBlocked, onRemindersChosen)
            AssigneeField(assignee, { assignee = it }, assignees, myName)
            CategoryField(categories, { categories = it }, onCreateCategory)
            ProjectField(project, projects, "", { project = it })
        }
    }
}
