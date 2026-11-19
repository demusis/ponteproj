package io.github.demusis.ponte

/** Progress of a save that closes its screen only after the library confirmed it (new task, note). */
internal data class SaveState(val busy: Boolean = false, val error: String = "")

/**
 * Rules of those saves, free of Android so they can be tested: one save at a time (a second tap
 * starts nothing), a failure keeps the screen open with the reason so the same form can be sent
 * again, and only the screen that started a save is closed by its result.
 */
internal object SaveGate {
    /** The state after a tap, or null when a save is already running (the tap is ignored). */
    fun begin(current: SaveState): SaveState? = if (current.busy) null else SaveState(busy = true)

    /** A save the library refused: the form stays, with the reason, ready for another try. */
    fun failed(message: String): SaveState = SaveState(busy = false, error = message)

    /** Confirmed locally: nothing left to show. */
    fun done(): SaveState = SaveState()

    /** Closes [route] only when it is still on screen (it may have been replaced meanwhile). */
    fun <R> close(stack: MutableList<R>, route: R): Boolean = stack.remove(route)
}
