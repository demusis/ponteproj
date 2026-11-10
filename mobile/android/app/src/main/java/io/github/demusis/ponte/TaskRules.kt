package io.github.demusis.ponte

/**
 * What the person is typing on the task screen: the form, the task it started from (its version and
 * its values at that moment) and the [epoch] of the screen state it belongs to. Plain values, so the
 * rules below are tested without Android.
 */
internal data class TaskDraft(
    val epoch: Int,
    /** The version of the stored task this draft started from; every save sends this one. */
    val baseVersion: Long,
    val base: TaskForm,
    val form: TaskForm,
    /** The stored task is newer than [baseVersion] and the person has changes: nothing can be saved. */
    val conflict: Boolean = false
) {
    val changed: Boolean get() = form.changedFrom(base)

    fun toSave(taskId: Long) = TaskSave(taskId, baseVersion, base, form)

    /** Flat list of strings and numbers, for the saved state of the screen. */
    fun toList(): List<Any> = listOf(epoch, baseVersion, conflict) + base.toList() + form.toList()

    companion object {
        fun fromList(l: List<Any>): TaskDraft? = try {
            TaskDraft(
                l[0] as Int, l[1] as Long, TaskForm.fromList(l.subList(3, 9).map { it as String }),
                TaskForm.fromList(l.subList(9, 15).map { it as String }), l[2] as Boolean
            )
        } catch (_: Throwable) {
            null
        }
    }
}

/** What a save sends: the task, the version the form started from, and only what differs from it. */
internal data class TaskSave(val taskId: Long, val version: Long, val base: TaskForm, val form: TaskForm) {
    val changes: Map<String, String> get() = form.changes(base)
}

internal fun TaskForm.toList(): List<String> = listOf(title, action, priority, dueDate, assignee, notes)

internal fun TaskForm.Companion.fromList(l: List<String>) = TaskForm(l[0], l[1], l[2], l[3], l[4], l[5])

/** Decisions of the task screen. */
internal object TaskRules {
    /**
     * The text of the core's conflict answer (mobile/rust/src/lib.rs, `TASK_CHANGED`); it also says
     * what the draft rule below detects, so the person reads one sentence for both.
     */
    const val CHANGED = "Esta tarefa foi alterada depois que você a abriu. Abra-a de novo para ver a versão atual."

    /** The task the screen of route [routeId] may show: only that one, never another that is loaded. */
    fun shown(routeId: Long, state: TaskEditState): TaskDetail? = state.task?.takeIf { it.id == routeId }

    /** A new draft for [task], at the screen state's [epoch]. */
    fun start(task: TaskDetail, epoch: Int) = TaskDraft(epoch, task.version, task.form, task.form)

    /**
     * Fits [draft] to the stored [task] just loaded:
     * - the person asked for the current version (other [epoch]): start over from it;
     * - same version: the draft stands;
     * - newer version and nothing typed: start over from it (silently);
     * - newer version and something typed: keep the text, flag the conflict (a save would send the
     *   old version and be refused).
     */
    fun reconcile(draft: TaskDraft, task: TaskDetail, epoch: Int): TaskDraft = when {
        draft.epoch != epoch -> start(task, epoch)
        draft.baseVersion == task.version -> draft.copy(conflict = false)
        !draft.changed -> start(task, epoch)
        else -> draft.copy(conflict = true)
    }

    fun isConflict(message: String) = message == CHANGED

    /**
     * The stack after opening task [id]: at most one task screen, always the new one on top. What was
     * under the old one stays, so Back goes to where the person was (a ficha), never to an editor
     * that belongs to another task.
     */
    fun withTask(stack: List<Route>, id: Long): List<Route> =
        stack.filter { it !is Route.EditTask && it !is Route.Pick } + Route.EditTask(id)

    /** The stack coming back after the process died: only the top-most task screen, and its own list. */
    fun restored(stack: List<Route>): List<Route> {
        val editor = stack.lastOrNull { it is Route.EditTask } as Route.EditTask?
        return stack.filter {
            when (it) {
                is Route.EditTask -> it == editor
                is Route.Pick -> it.taskId == editor?.id
                else -> true
            }
        }
    }
}
