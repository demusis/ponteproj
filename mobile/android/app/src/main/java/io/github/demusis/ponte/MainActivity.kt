package io.github.demusis.ponte

import android.content.Intent
import android.os.Bundle
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
        setContent { PonteTheme { AppRoot() } }
    }

    override fun onResume() {
        super.onResume()
        live = this
        SyncRunner.listener = { Controller.render() }
        Controller.render()
    }

    override fun onPause() {
        SyncRunner.listener = null
        super.onPause()
    }

    override fun onDestroy() {
        if (live === this) live = null
        super.onDestroy()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        Controller.save(outState)
    }

    @Deprecated("The classic result callback: no extra library needed.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        when (requestCode) {
            REQ_PICK -> if (resultCode == RESULT_OK) data?.data?.let { Controller.onFilePicked(it) }
            REQ_PHOTO -> Controller.onCamera(resultCode == RESULT_OK)
        }
    }

    companion object {
        const val REQ_PICK = 1
        const val REQ_PHOTO = 2

        /** The Activity on screen, for what needs one (pickers, fingerprint prompt). */
        @Volatile
        internal var live: MainActivity? = null
    }
}
