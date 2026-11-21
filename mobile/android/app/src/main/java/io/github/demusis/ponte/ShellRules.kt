package io.github.demusis.ponte

// Pure rules of the frame of the main areas (title, sync dot, filters, task rows), tested without Android.

/** The title of the Tarefas tab doubles as the selector of its four areas (Tarefas, Projetos, Atendimentos, Recursos). */
internal object AreaSelector {
    /** The name of the area [segment] (an id of [TasksSegments]); an unknown id reads as Tarefas. */
    fun title(segment: Int): String = TasksSegments.LABELS[segment] ?: TasksSegments.LABELS.getValue(TasksSegments.TASKS)

    /** What the screen reader says for the title: the area and that a tap changes it. */
    fun description(segment: Int): String = "Área: ${title(segment)}. Toque para trocar."

    /** The areas in the order of the menu: (id, name). */
    fun options(): List<Pair<Int, String>> = TasksSegments.ORDER.map { it to title(it) }

    /** What the screen reader says for one entry of the menu: the current one is marked. */
    fun optionDescription(segment: Int, current: Int): String =
        if (segment == current) "${title(segment)}, área atual" else title(segment)

    /** The area the frame shows (the flags of [ShellState] mirror the controller's segment). */
    fun of(state: ShellState): Int = when {
        state.attendances -> TasksSegments.ATTENDANCES
        state.resources -> TasksSegments.RESOURCES
        state.projects -> TasksSegments.PROJECTS
        else -> TasksSegments.TASKS
    }

    /** The title of the frame for the tab (0 Hoje, 1 Tarefas, 2 Documentos, 3 E-mail) of [state]. */
    fun heading(state: ShellState): String = when (state.tab) {
        0 -> "Hoje"
        1 -> title(of(state))
        2 -> "Documentos"
        3 -> "E-mail"
        else -> ""
    }

    /** Only the Tarefas tab has areas to choose from. */
    fun selectable(state: ShellState) = state.tab == 1
}

/** The sync state as a dot after the title, and when its text line is worth the space. */
internal object SyncDot {
    /** How long the text stays after a sync ends, or after a tap on the dot. */
    const val SHOW_MS = 4000L

    /** The status text of the frame ("Sincronizado · 14:32"). */
    fun text(state: ShellState): String = state.statusLine.ifEmpty { "Ainda não sincronizado" }

    /**
     * The text line shows while syncing, whenever the state is not the quiet "Sincronizado" (error,
     * unsent changes, a refusal, nothing synced yet), for a few seconds after a sync ended
     * ([recentlyEnded]) and while the dot was tapped ([peek]).
     */
    fun lineVisible(state: ShellState, recentlyEnded: Boolean, peek: Boolean): Boolean =
        state.syncing || peek || recentlyEnded || state.statusKind != StatusKind.OK

    /** "Sincronizar agora" can be tapped: the key of the device is fine and no sync is running (same rule as the old button). */
    fun canSync(state: ShellState): Boolean = state.syncEnabled && !state.syncing

    /** The label of that menu item: while it runs, it says so (that is why it cannot be tapped). */
    fun syncItemLabel(state: ShellState): String = if (state.syncing) "Sincronizando…" else "Sincronizar agora"

    /** What the screen reader says for the dot: the whole status text, and its kind. */
    fun description(state: ShellState): String {
        val kind = when {
            state.syncing -> "Sincronizando"
            state.statusKind == StatusKind.ERROR -> "Sincronização com problema"
            state.statusKind == StatusKind.WAITING -> "Sincronização pendente"
            else -> "Sincronização"
        }
        val text = text(state)
        return if (text.startsWith(kind)) text else "$kind: $text"
    }
}

/** The "Filtro: Estudo · Projeto X" line over a filtered list, and what the filter icon says. */
internal object FilterSummary {
    /** The most category names the line spells out; the others are counted. */
    const val MAX_NAMES = 2

    /** Names of the chosen categories ("Sem categoria" for the marker), the ones unknown to the library left out. */
    fun categoryNames(chosen: Set<String>, all: List<Category>): List<String> {
        val byUid = all.associateBy { it.uid }
        return chosen.sortedWith(compareBy({ it != CategoryRules.NONE }, { byUid[it]?.let { c -> CategoryRules.nameKey(c.name) } ?: "" }))
            .mapNotNull { if (it == CategoryRules.NONE) "Sem categoria" else byUid[it]?.name }
    }

    /** The project part ("Sem projeto", a name) or empty for none. */
    fun projectName(chosen: String, projects: List<ProjectItem>): String =
        if (chosen.isEmpty()) "" else ProjectRules.filterLabel(chosen, projects)

    /** "Filtro: Estudo · Projeto X"; empty when nothing is chosen. */
    fun line(categoryNames: List<String>, project: String, document: String = ""): String {
        val shown = categoryNames.take(MAX_NAMES).joinToString(", ")
        val more = (categoryNames.size - MAX_NAMES).coerceAtLeast(0)
        val categories = if (more > 0) "$shown +$more" else shown
        val parts = listOf(categories, project, document).filter { it.isNotEmpty() }
        return if (parts.isEmpty()) "" else "Filtro: " + parts.joinToString(" · ")
    }

    fun isActive(categoryChosen: Set<String>, projectChosen: String, documentChosen: Long? = null) = categoryChosen.isNotEmpty() || projectChosen.isNotEmpty() || documentChosen != null

    /** How many open tasks the three filters leave. */
    fun openCount(tasks: List<TaskItem>, categoryChosen: Set<String>, projectChosen: String, documentChosen: Long?): Int =
        tasks.count {
            !it.done && CategoryRules.matches(it.categories, categoryChosen) && ProjectRules.matchesTask(it.projectUid, projectChosen) &&
                TaskDocumentFilter.matches(it.documentId, documentChosen)
        }

    /** The button that closes the sheet says what the filter leaves; "Pronto" where nothing is counted. */
    fun doneLabel(count: Int?): String = when (count) {
        null -> "Pronto"
        0 -> "Nenhuma tarefa em aberto"
        1 -> "Ver 1 tarefa"
        else -> "Ver $count tarefas"
    }

    /** What a closed section shows at its right: the choice, or "Todos". */
    fun sectionValue(chosenLabel: String): String = chosenLabel.ifEmpty { "Todos" }

    /** The sections the sheet has: categories when the library has any (or one is chosen), projects likewise. */
    fun hasCategories(all: List<Category>, chosen: Set<String>) = all.isNotEmpty() || chosen.isNotEmpty()
    fun hasProjects(projects: List<ProjectItem>, chosen: String) = projects.isNotEmpty() || chosen.isNotEmpty()

    /** What the filter icon says to the screen reader. */
    fun iconDescription(active: Boolean) = if (active) "Filtros, há um filtro ativo" else "Filtros"
}

/** The pieces of the second line of a task row. */
internal object TaskRowText {
    /** A priority worth flagging in a row: only above normal. [key] is the core's key, [word] the short word. */
    data class Mark(val key: String, val word: String)

    /** "Examinar · MANUAL-2026/0003": the kind and the document number; no "Sem documento". */
    fun lead(task: TaskItem): String =
        listOf(task.actionLabel, if (task.documentId > 0) task.documentNumber else "").filter { it.isNotEmpty() }.joinToString(" · ")

    /** "alta" or "urgente" for an open task; null for normal, low and done ones. */
    fun mark(task: TaskItem): Mark? {
        if (task.done) return null
        return when (task.priority) {
            "urgente" -> Mark("urgente", "urgente")
            "alta" -> Mark("alta", "alta")
            else -> null
        }
    }

    /**
     * How many of [widths] (the widths of the category labels, in order) fit in [room], each one taking [gap]
     * before it, when a left-out count of any size takes [moreWidth]. At least what fits; never more than there are.
     */
    fun fitting(widths: List<Float>, room: Float, gap: Float, moreWidth: Float): Int {
        var used = 0f
        var shown = 0
        for ((i, w) in widths.withIndex()) {
            val next = used + gap + w
            val last = i == widths.lastIndex
            // Another label after this one needs the "+N"; the last one needs nothing more.
            val needed = if (last) next else next + gap + moreWidth
            if (needed <= room) {
                used = next
                shown = i + 1
            } else break
        }
        // All of them fit with nothing left: no "+N". Otherwise the check above kept room for it.
        return shown
    }

    /** The whole row for the screen reader: title, deadline, kind, number, priority, categories and project. */
    fun description(task: TaskItem, categoryNames: List<String>, showProject: Boolean): String {
        val parts = mutableListOf(task.title)
        if (task.done) parts += "concluída"
        else if (task.dueChip.isNotEmpty()) parts += (if (task.overdue) "atrasada, " else "prazo ") + task.dueChip
        parts += lead(task)
        mark(task)?.let { parts += "prioridade ${it.word}" }
        if (categoryNames.isNotEmpty()) parts += "categorias " + categoryNames.joinToString(", ")
        if (showProject && task.projectName.isNotEmpty()) parts += "projeto ${task.projectName}"
        return parts.filter { it.isNotEmpty() }.joinToString(". ")
    }
}

/** Pull to refresh: how long it waits for the sync to show itself before giving up the spinner. */
internal object PullRules {
    /** A refused sync (no Wi-Fi, not set up) never starts; the spinner stops after this long (the reason is said at the bottom). */
    const val START_WAIT_MS = 1500L

    /** A sync that never ends does not hold the spinner for ever. */
    const val MAX_SPIN_MS = 90_000L

    /** Hoje, Tarefas (all four areas) and Documentos refresh; E-mail has its own list and button. */
    fun enabled(tab: Int) = tab in 0..2
}

/** The sections of the filter sheet that open and close (Projeto, Documento); one is open at a time. */
internal enum class FilterSection { NONE, PROJECT, DOCUMENT }

internal object FilterSections {
    /** A section that is the only thing in the sheet starts open; otherwise all start closed, showing their choice. */
    fun initial(hasCategories: Boolean, hasProjects: Boolean, hasDocuments: Boolean): FilterSection = when {
        hasCategories || (hasProjects && hasDocuments) -> FilterSection.NONE
        hasProjects -> FilterSection.PROJECT
        hasDocuments -> FilterSection.DOCUMENT
        else -> FilterSection.NONE
    }

    fun toggle(open: FilterSection, tapped: FilterSection): FilterSection = if (open == tapped) FilterSection.NONE else tapped

    /** One choice at a time: tapping the chosen one again clears it. */
    fun <T> pick(chosen: T, tapped: T, none: T): T = if (chosen == tapped) none else tapped
}

/** Which tabs and areas have a filter icon. */
internal object FilterIcon {
    /**
     * The icon shows whenever the list has something to filter by (a category in the library or a project), also with
     * no filter on; it is hidden only when there is nothing to filter by and no filter is left to clear.
     */
    fun tasksVisible(
        state: ShellState, categories: List<Category>, categoryChosen: Set<String>, projects: List<ProjectItem>, projectChosen: String,
        documents: List<Doc> = emptyList(), documentChosen: Long? = null
    ) = forTasks(state) && (FilterSummary.hasCategories(categories, categoryChosen) || FilterSummary.hasProjects(projects, projectChosen) || documents.isNotEmpty() || documentChosen != null)

    fun documentsVisible(state: ShellState, categories: List<Category>, categoryChosen: Set<String>) =
        forDocuments(state) && FilterSummary.hasCategories(categories, categoryChosen)

    /** The category/project filter of the task list; the category filter of the document list. */
    fun forTasks(state: ShellState) = state.tab == 1 && AreaSelector.of(state) == TasksSegments.TASKS
    fun forDocuments(state: ShellState) = state.tab == 2
}
