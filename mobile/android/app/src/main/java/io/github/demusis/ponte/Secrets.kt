package io.github.demusis.ponte

import android.app.Activity
import android.content.Context
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.InvalidKeyException
import java.security.KeyStore
import java.security.UnrecoverableKeyException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Optional storage of the sync passphrase, encrypted with a Keystore key that needs the
 * fingerprint (or the device PIN on Android 11+) for each use. Only on Android 9+.
 */
internal object Secrets {
    private const val ALIAS = "ponte_frase"
    private const val CT = "pp_ct"
    private const val IV = "pp_iv"

    /** UI thread only: a prompt is on screen. */
    private var busy = false

    fun supported() = Build.VERSION.SDK_INT >= 28

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("ponte", Context.MODE_PRIVATE)

    fun isRemembered(ctx: Context) =
        supported() && prefs(ctx).contains(CT) && prefs(ctx).contains(IV)

    private fun keyStore() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    fun forget(ctx: Context) {
        prefs(ctx).edit().remove(CT).remove(IV).commit()
        try {
            val ks = keyStore()
            if (ks.containsAlias(ALIAS)) ks.deleteEntry(ALIAS)
        } catch (_: Throwable) {
        }
    }

    private fun createKey() {
        val ks = keyStore()
        if (ks.containsAlias(ALIAS)) ks.deleteEntry(ALIAS)
        val b = KeyGenParameterSpec.Builder(
            ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
        if (Build.VERSION.SDK_INT >= 30) {
            b.setUserAuthenticationParameters(
                0, KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL
            )
        }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(b.build())
        gen.generateKey()
    }

    private fun prompt(
        activity: Activity, cipher: Cipher, title: String,
        onOk: (Cipher) -> Unit, onErr: (String) -> Unit
    ) {
        val exec = activity.mainExecutor
        val b = BiometricPrompt.Builder(activity)
            .setTitle(title)
            .setDescription("Use a digital ou o PIN do aparelho.")
        if (Build.VERSION.SDK_INT >= 30) {
            b.setAllowedAuthenticators(
                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
            )
        } else {
            b.setNegativeButton("Cancelar", exec) { _, _ -> onErr("Cancelado.") }
        }
        val cb = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult?) {
                val c = result?.cryptoObject?.cipher
                if (c == null) onErr("Não foi possível liberar a chave.") else onOk(c)
            }

            override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) {
                onErr(errString?.toString() ?: "Cancelado.")
            }
        }
        b.build().authenticate(BiometricPrompt.CryptoObject(cipher), CancellationSignal(), exec, cb)
    }

    /** Encrypts pass after the user confirms with fingerprint/PIN. done(ok, message). */
    fun remember(activity: Activity, pass: String, done: (Boolean, String) -> Unit) {
        if (!supported()) { done(false, "Não disponível neste aparelho."); return }
        if (busy) { done(false, "Aguarde a verificação em andamento."); return }
        busy = true
        try {
            createKey()
            val key = keyStore().getKey(ALIAS, null) as SecretKey
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key)
            prompt(activity, cipher, "Guardar a frase-senha", { c ->
                busy = false
                try {
                    val ct = c.doFinal(pass.toByteArray(Charsets.UTF_8))
                    val ok = prefs(activity).edit()
                        .putString(CT, Base64.encodeToString(ct, Base64.NO_WRAP))
                        .putString(IV, Base64.encodeToString(c.iv, Base64.NO_WRAP))
                        .commit()
                    if (ok) done(true, "Frase-senha guardada.") else {
                        forget(activity)
                        done(false, "Não foi possível guardar.")
                    }
                } catch (e: Throwable) {
                    forget(activity)
                    done(false, e.message ?: e.toString())
                }
            }, { msg ->
                busy = false
                forget(activity)
                done(false, msg)
            })
        } catch (e: Throwable) {
            busy = false
            forget(activity)
            done(false, "Não foi possível proteger com digital ou PIN. Confira se o aparelho tem bloqueio de tela. (" +
                (e.message ?: e.toString()) + ")")
        }
    }

    /** Decrypts the stored passphrase. result(pass, null) or result(null, message). */
    fun unlock(activity: Activity, result: (String?, String?) -> Unit) {
        if (!isRemembered(activity)) { result(null, "Nenhuma frase-senha guardada."); return }
        if (busy) { result(null, "Aguarde a verificação em andamento."); return }
        busy = true
        val invalid = "A chave guardada não pôde ser usada (a digital ou o PIN pode ter mudado). Digite a frase-senha de novo."
        try {
            val p = prefs(activity)
            val ct = Base64.decode(p.getString(CT, "") ?: "", Base64.NO_WRAP)
            val iv = Base64.decode(p.getString(IV, "") ?: "", Base64.NO_WRAP)
            val key = keyStore().getKey(ALIAS, null) as? SecretKey
                ?: throw UnrecoverableKeyException("key missing")
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
            prompt(activity, cipher, "Liberar a frase-senha", { c ->
                busy = false
                try {
                    result(String(c.doFinal(ct), Charsets.UTF_8), null)
                } catch (e: InvalidKeyException) {
                    forget(activity)
                    result(null, invalid)
                } catch (e: UnrecoverableKeyException) {
                    forget(activity)
                    result(null, invalid)
                } catch (e: Throwable) {
                    result(null, e.message ?: e.toString())
                }
            }, { msg ->
                busy = false
                result(null, msg)
            })
        } catch (e: InvalidKeyException) {
            busy = false
            forget(activity)
            result(null, invalid)
        } catch (e: UnrecoverableKeyException) {
            busy = false
            forget(activity)
            result(null, invalid)
        } catch (e: Throwable) {
            busy = false
            result(null, e.message ?: e.toString())
        }
    }
}
