package io.github.demusis.ponte

/** How long the app may stay out of sight before it asks again. */
internal enum class LockDelay(val key: String, val millis: Long, val label: String) {
    NOW("agora", 0L, "Imediatamente"),
    ONE_MINUTE("1", 60_000L, "Depois de 1 minuto"),
    FIVE_MINUTES("5", 300_000L, "Depois de 5 minutos");

    companion object {
        val DEFAULT = ONE_MINUTE

        fun fromKey(key: String?): LockDelay = values().firstOrNull { it.key == key } ?: DEFAULT
    }
}

/**
 * The decisions of the app lock, with no Android types so they can be tested on the computer.
 *
 * The lock is an access gate in the interface: it keeps the library from being shown to
 * someone who picks up the unlocked phone. It is NOT encryption (the library is sealed by its
 * own Keystore key whatever this says) and it does not protect against someone who can unlock
 * the phone, or against a rooted device or a debugger.
 */
internal object LockPolicy {
    /** On by default when the phone has a secure screen lock; off otherwise. */
    fun defaultEnabled(deviceSecure: Boolean) = deviceSecure

    /**
     * The lock is in force only when it is on AND the phone has a secure screen lock; without
     * one the person could never prove anything, so the app simply opens.
     */
    fun active(enabled: Boolean, deviceSecure: Boolean) = enabled && deviceSecure

    /**
     * Whether the library must be hidden behind the lock screen.
     * [awayMillis] is the time spent out of sight (monotonic clock), or null when the app was
     * not out of sight since the last check. [coldStart] is a new process.
     */
    fun shouldLock(
        enabled: Boolean, deviceSecure: Boolean, delay: LockDelay, awayMillis: Long?, coldStart: Boolean
    ): Boolean {
        if (!active(enabled, deviceSecure)) return false
        if (coldStart) return true
        if (awayMillis == null) return false
        // A negative time means the clock restarted (the phone rebooted): treat it as long.
        if (awayMillis < 0) return true
        return delay.millis == 0L || awayMillis > delay.millis
    }

    /**
     * Unlocking can BE the prompt that releases the stored sync passphrase (one prompt, not
     * two) only when that prompt is the system one that takes the device PIN too (Android 11+),
     * a passphrase is stored and not yet in memory, and a sync is about to need it.
     */
    fun unlockReleasesPassphrase(
        sdk: Int, stored: Boolean, inMemory: Boolean, syncDue: Boolean, failedBefore: Boolean = false
    ) = sdk >= 30 && stored && !inMemory && syncDue && !failedBefore

    /**
     * The app left the front while a system prompt (its own or a passphrase one) was still
     * pending: the decision waits for the prompt. A success is a fresh authentication, so no
     * lock; a failure or cancel decides by the time spent away as usual.
     */
    fun lockAfterPrompt(
        enabled: Boolean, deviceSecure: Boolean, delay: LockDelay, awayMillis: Long?, promptSucceeded: Boolean
    ) = !promptSucceeded && shouldLock(enabled, deviceSecure, delay, awayMillis, coldStart = false)

    /**
     * Files handed to other apps are deleted on lock, at cold start and when the person comes
     * back after opening one. Photos of a "Registro por foto" in progress are the exception:
     * kept until the record is created or the form is closed; and while a record is being
     * saved the whole photos folder is left alone.
     */
    fun mayDeletePhoto(path: String, inProgress: Set<String>, saving: Boolean) =
        !saving && path !in inProgress
}
