package io.github.demusis.ponte

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Toast

/**
 * Receives what another app shares ("Compartilhar" > "Guardar no Ponte"), one item at a time.
 *
 * Why a separate entry and not [MainActivity] itself: the launcher Activity has to stay as it is
 * (one instance, no intent filters of its own), and this one can be as small as possible. It shows
 * nothing and is gone at once: it validates the intent ([ShareRules.check]), keeps only what it
 * needs in memory ([Intake.offer]: the Uri, the capped text and subject), and brings [MainActivity]
 * forward with a marker intent that carries no content, only the read grant for the Uri.
 * Nothing the sender wrote is trusted: not the type, not the name, not the text.
 */
class ShareReceiverActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        try {
            handle(intent)
        } catch (_: Throwable) {
            Toast.makeText(this, ShareRules.MSG_BAD_TYPE, Toast.LENGTH_LONG).show()
        }
        finish()
    }

    @Suppress("DEPRECATION")
    private fun handle(i: Intent?) {
        if (i == null) return
        val action = i.action
        val streams = LinkedHashSet<Uri>()
        var badStream = false
        try {
            when (val extra = i.extras?.get(Intent.EXTRA_STREAM)) {
                null -> {}
                is Uri -> streams.add(extra)
                else -> badStream = true
            }
            val clip: ClipData? = i.clipData
            if (clip != null) for (n in 0 until clip.itemCount) clip.getItemAt(n).uri?.let { streams.add(it) }
        } catch (_: Throwable) {
            // An extra that cannot be read is not a Uri we can use.
            badStream = true
        }
        val uri = streams.firstOrNull()
        val count = streams.size + (if (badStream) 1 else 0)
        val text = try {
            ShareRules.plainText(i.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString(), AttachLimits.maxTextBytes)
        } catch (_: Throwable) {
            ""
        }
        val subject = try {
            ShareRules.oneLine(i.getCharSequenceExtra(Intent.EXTRA_SUBJECT)?.toString(), AttachLimits.maxTitleBytes)
        } catch (_: Throwable) {
            ""
        }
        // A stream that is not a usable Uri counts as a stream with no scheme: refused as not content.
        val verdict = ShareRules.check(
            action, i.type, if (badStream) null else uri?.scheme, if (badStream) null else uri?.authority,
            count, text.isNotEmpty(), PonteFiles.AUTHORITY
        )
        if (!verdict.ok) {
            Toast.makeText(this, verdict.message, Toast.LENGTH_LONG).show()
            return
        }
        Intake.offer(Intake.Offer(action, i.type, uri, count, text, subject))
        val forward = Intent(this, MainActivity::class.java).apply {
            setAction(Intake.ACTION_RECEIVED)
            // No CLEAR_TOP: whatever sits above the main screen (camera, picker, unlock prompt) stays.
            // If the main screen is on top it gets the marker (onNewIntent); if not, the instance this
            // makes finishes at once and the share is taken when the person is back (see MainActivity).
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            if (uri != null) {
                // The sender's grant is given on to the app's main screen, for the reading that follows.
                clipData = ClipData.newRawUri("", uri)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
        }
        startActivity(forward)
    }
}
