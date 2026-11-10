package io.github.demusis.ponte

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal

/**
 * Proof that the person is the owner of the phone, with the phone's own screen lock
 * (fingerprint/face, or PIN/pattern/password). Nothing is stored and no key is involved: it
 * only answers "yes" or "no" to the interface. UI thread only.
 */
internal object AppLock {
    private var signal: CancellationSignal? = null
    private var pendingOk: (() -> Unit)? = null
    private var pendingFail: (() -> Unit)? = null

    /** The phone has a PIN, pattern, password or biometric screen lock. */
    fun deviceSecure(ctx: Context): Boolean = try {
        ctx.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
    } catch (_: Throwable) {
        false
    }

    /**
     * Asks for the screen lock. Exactly one of [onOk] / [onFail] runs (cancel, error and "no
     * screen lock" are all [onFail]). Android 10+: the system prompt that takes biometrics or
     * the device credential. Older: the system "confirm your PIN" screen, whose answer comes
     * to [onResult].
     */
    fun verify(a: Activity, title: String, onOk: () -> Unit, onFail: () -> Unit) {
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                val b = BiometricPrompt.Builder(a)
                    .setTitle(title)
                    .setDescription("Use a digital ou o bloqueio de tela do celular.")
                if (Build.VERSION.SDK_INT >= 30) {
                    b.setAllowedAuthenticators(
                        BiometricManager.Authenticators.BIOMETRIC_WEAK or
                            BiometricManager.Authenticators.DEVICE_CREDENTIAL
                    )
                } else {
                    @Suppress("DEPRECATION")
                    b.setDeviceCredentialAllowed(true)
                }
                val cs = CancellationSignal()
                signal = cs
                b.build().authenticate(cs, a.mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult?) {
                        signal = null
                        onOk()
                    }

                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) {
                        signal = null
                        onFail()
                    }
                })
            } else {
                // Android 8 and 9: the platform prompt of Android 9 cannot take the PIN, so the
                // system confirmation screen is used.
                val km = a.getSystemService(KeyguardManager::class.java)
                @Suppress("DEPRECATION")
                val i = km?.createConfirmDeviceCredentialIntent(title, "Use o bloqueio de tela do celular.")
                if (i == null) {
                    onFail()
                    return
                }
                pendingOk = onOk
                pendingFail = onFail
                @Suppress("DEPRECATION")
                a.startActivityForResult(i, MainActivity.REQ_UNLOCK)
            }
        } catch (_: Throwable) {
            pendingOk = null
            pendingFail = null
            onFail()
        }
    }

    /** The answer of the system confirmation screen (Android 8 and 9). */
    fun onResult(ok: Boolean) {
        val yes = pendingOk
        val no = pendingFail
        pendingOk = null
        pendingFail = null
        if (ok) yes?.invoke() else no?.invoke()
    }

    /** The Activity is going away: forget what was waiting. */
    fun cancel() {
        pendingOk = null
        pendingFail = null
        try { signal?.cancel() } catch (_: Throwable) { }
        signal = null
    }
}
