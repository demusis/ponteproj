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

    /**
     * True for an instance that was made only because something started the launcher Activity while
     * another one was alive but not on top (the camera, the file picker or a viewer in front: the
     * deadline notice, a share). It does nothing but pass the request on and finish; see [onCreate].
     */
    private var duplicate = false

    override fun onCreate(savedInstanceState: Bundle?) {
        // One MainActivity only. The launch mode stays "standard" on purpose: singleTask would
        // finish the camera, the picker or the unlock prompt sitting above the live instance
        // (their result would come back cancelled and the photo would be lost). Instead, an
        // instance made while a live one exists is a duplicate: the request goes to the live state
        // through the same guarded paths as onNewIntent, and the duplicate finishes, so what was
        // in front (the camera...) is in front again and nothing of the started Controller is reset.
        // A recreation (saved state present) is never a duplicate.
        val other = live
        if (savedInstanceState == null && other != null && other !== this &&
            !other.isFinishing && !other.isDestroyed && Controller.isStarted()
        ) {
            duplicate = true
            super.onCreate(savedInstanceState)
            if (intent?.action == NoticeScheduler.ACTION_TODAY) Controller.showToday()
            // A share marker needs nothing here: its offer waits in memory and the live instance
            // takes it when the person is back in front of it (onResume).
            finish()
            return
        }
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        live = this
        Controller.attach(this, savedInstanceState)
        applySecure(Controller.secureScreen())
        setContent { PonteTheme { AppRoot() } }
        // Only a fresh start looks at the intent: a recreated Activity (rotation, process restored
        // from the saved state) must not take a share again. The share itself is taken once from
        // memory (see Intake.offer), so even a repeated marker finds nothing.
        if (savedInstanceState == null) handleReceived(intent)
    }

    /** The marker the share receiver sends: the content itself waits in [Intake], not in the intent. */
    private fun handleReceived(i: Intent?) {
        if (i?.action == Intake.ACTION_RECEIVED) Intake.receive()
    }

    override fun onStart() {
        super.onStart()
        if (duplicate) return
        live = this
        Controller.onActivityStart()
    }

    override fun onResume() {
        super.onResume()
        if (duplicate) return
        live = this
        SyncRunner.listener = { Controller.render() }
        Controller.render()
        Controller.onActivityResumed()
        // A share that came while something else was in front is delivered now, that the person is back.
        Intake.receive()
    }

    override fun onPause() {
        if (duplicate) { super.onPause(); return }
        SyncRunner.listener = null
        super.onPause()
    }

    override fun onStop() {
        if (duplicate) { super.onStop(); return }
        Controller.onActivityStop(isChangingConfigurations)
        Intake.onHostStop(isChangingConfigurations)
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
        if (duplicate) { super.onDestroy(); return }
        // An old instance destroyed after a new one started must not touch the new one's prompt.
        if (live === this || live == null) Controller.onActivityDestroyed()
        if (live === this) live = null
        super.onDestroy()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        if (!duplicate) Controller.save(outState)
    }

    /** The deadline notice was tapped while the app was already open: it goes to Hoje. */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (duplicate) return
        setIntent(intent)
        if (intent.action == NoticeScheduler.ACTION_TODAY) Controller.showToday()
        handleReceived(intent)
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
            REQ_ATTACH_PHOTO -> Intake.onAttachPhotoResult(resultCode == RESULT_OK)
            REQ_ATTACH_FILE -> Intake.onAttachPicked(if (resultCode == RESULT_OK) data?.data else null)
        }
    }

    companion object {
        const val REQ_PICK = 1
        const val REQ_PHOTO = 2
        const val REQ_UNLOCK = 3
        const val REQ_NOTICE = 4
        const val REQ_ATTACH_PHOTO = 5
        const val REQ_ATTACH_FILE = 6

        /** The Activity on screen, for what needs one (pickers, fingerprint prompt). */
        @Volatile
        internal var live: MainActivity? = null
    }
}
