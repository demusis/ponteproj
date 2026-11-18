package io.github.demusis.ponte

internal enum class SyncRefusal { NO_FILE, NO_WIFI, PASSPHRASE_MISSING, NOT_FOREGROUND, PASSPHRASE_CANCELLED, BUSY }

internal data class SyncFeedbackResult(val status: String, val message: String?, val mobileAction: Boolean = false)

internal object SyncFeedback {
    fun decide(reason: SyncRefusal, auto: Boolean): SyncFeedbackResult {
        val text = when (reason) {
            SyncRefusal.NO_FILE -> "Escolha primeiro o arquivo ponte-sync.ponte em Ajustes."
            SyncRefusal.NO_WIFI -> "Sem Wi-Fi: sincronização adiada. Para usar dados móveis, ative em Ajustes."
            SyncRefusal.PASSPHRASE_MISSING -> "Informe a senha de sincronização em Ajustes para sincronizar."
            SyncRefusal.NOT_FOREGROUND -> "Abra o aplicativo para liberar a senha e sincronizar."
            SyncRefusal.PASSPHRASE_CANCELLED -> "Sincronização cancelada: a senha não foi liberada."
            SyncRefusal.BUSY -> "Já há uma sincronização em andamento."
        }
        return SyncFeedbackResult(text, if (auto) null else text, !auto && reason == SyncRefusal.NO_WIFI)
    }
}
