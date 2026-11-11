package io.github.demusis.ponte

import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

/**
 * The only screen of the app, drawn with Compose. What it shows and does lives in
 * [Controller], which outlives this Activity; here are only the pieces that need an Activity:
 * the content, the results of the file picker and of the camera, and the saved state.
 */
class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        live = this
        Controller.attach(this, savedInstanceState)
        applySecure(Controller.secureScreen())
        setContent { PonteTheme { AppRoot() } }
    }

    override fun onStart() {
        super.onStart()
        live = this
        Controller.onActivityStart()
    }

    override fun onResume() {
        super.onResume()
        live = this
        SyncRunner.listener = { Controller.render() }
        Controller.render()
        Controller.onActivityResumed()
    }

    override fun onPause() {
        SyncRunner.listener = null
        super.onPause()
    }

    override fun onStop() {
        Controller.onActivityStop(isChangingConfigurations)
        super.onStop()
    }

    /**
     * No screenshots and no preview in the recent apps list while on (the default): the system
     * blanks the window for both. Applied at start and again when the switch changes.
     */
    fun applySecure(on: Boolean) {
        if (on) window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }

    override fun onDestroy() {
        // An old instance destroyed after a new one started must not touch the new one's prompt.
        if (live === this || live == null) Controller.onActivityDestroyed()
        if (live === this) live = null
        super.onDestroy()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        Controller.save(outState)
    }

    /** The deadline notice was tapped while the app was already open: it goes to Hoje. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (intent.action == NoticeScheduler.ACTION_TODAY) Controller.showToday()
    }

    /** The system asks for the permission to show notifications (Android 13 or newer). */
    fun askNoticePermission() {
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), REQ_NOTICE)
        }
    }

    @Deprecated("The classic permission callback, as for the results above.")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        @Suppress("DEPRECATION")
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_NOTICE) {
            Controller.onNoticePermission(grantResults.isNotEmpty() && grantResults[0] == android.content.pm.PackageManager.PERMISSION_GRANTED)
        }
    }

    @Deprecated("The classic result callback: no extra library needed.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        when (requestCode) {
            REQ_PICK -> if (resultCode == RESULT_OK) data?.data?.let { Controller.onFilePicked(it) }
            REQ_PHOTO -> Controller.onCamera(resultCode == RESULT_OK)
            REQ_UNLOCK -> AppLock.onResult(resultCode == RESULT_OK)
        }
    }

    companion object {
        const val REQ_PICK = 1
        const val REQ_PHOTO = 2
        const val REQ_UNLOCK = 3
        const val REQ_NOTICE = 4

        /** The Activity on screen, for what needs one (pickers, fingerprint prompt). */
        @Volatile
        internal var live: MainActivity? = null
    }
}
