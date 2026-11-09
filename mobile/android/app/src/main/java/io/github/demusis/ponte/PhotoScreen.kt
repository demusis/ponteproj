package io.github.demusis.ponte

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Intent
import android.os.Bundle
import android.provider.MediaStore
import android.view.View
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.ImageView
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import org.json.JSONObject
import java.io.File

/** Photo record: a few fields plus a picture taken by the camera app. */
internal class PhotoScreen(private val a: MainActivity) : Screen {
    private val ui = a.ui
    private val scroll = ScrollView(a)
    private val box = ui.vbox(12)

    private var kinds: List<Pair<String, String>> = listOf("papel" to "Papel")
    private var wantedKind = "papel"
    private var dateIso = ""
    private var photoPath: String? = null
    private var newPath: String? = null
    private var saving = false

    private val kindSpinner = Spinner(a)
    private val title = ui.edit("Título (obrigatório)")
    private val origin = ui.edit("Origem")
    private val text = ui.edit("Texto (opcional)", multiline = true)
    private val dateBtn: Button = ui.button("Data: sem data") {}
    private val thumb = ImageView(a).apply { adjustViewBounds = true; maxHeight = ui.dp(260) }
    private val progress: TextView = ui.text("", 13f, Colors.GRAY)
    private val saveBtn: Button = ui.button("Salvar") { save() }

    override val view: View get() = scroll

    init {
        scroll.addView(box)
        box.addView(ui.text("Registro por foto", 20f, bold = true))
        box.addView(ui.text("Tipo", 13f, Colors.GRAY))
        box.addView(kindSpinner, ui.wide())
        box.addView(title, ui.wide())
        box.addView(origin, ui.wide())
        dateBtn.isAllCaps = false
        dateBtn.setOnClickListener {
            ui.pickDate(dateIso) { d -> dateIso = d; updateDate() }
        }
        box.addView(dateBtn, ui.wide())
        val noDate = ui.button("Sem data") { dateIso = ""; updateDate() }
        noDate.isAllCaps = false
        box.addView(noDate, ui.wide())
        box.addView(text, ui.wide())
        box.addView(ui.button("Tirar foto") { takePhoto() }, ui.wide())
        box.addView(thumb, ui.wide())
        box.addView(progress, ui.wide())
        box.addView(saveBtn, ui.wide())
        box.addView(ui.button("Cancelar") { a.goBack() }, ui.wide())
        setKinds()
        SyncRunner.background({ Api.choices() }, { ch ->
            val k = ch.optJSONArray("manual_kinds").objs().map { it.s("key") to it.s("label") }
            if (k.isNotEmpty()) {
                kinds = k
                setKinds()
            }
        })
    }

    private fun setKinds() {
        kindSpinner.adapter = ArrayAdapter(
            a, android.R.layout.simple_spinner_dropdown_item, kinds.map { it.second }
        )
        val i = kinds.indexOfFirst { it.first == wantedKind }
        kindSpinner.setSelection(if (i >= 0) i else 0)
    }

    private fun updateDate() {
        dateBtn.text = if (dateIso.isEmpty()) "Data: sem data" else "Data: $dateIso"
    }

    // ------------------------------------------------------------ state across process death

    fun saveState(b: Bundle) {
        b.putString("photo", photoPath)
        b.putString("new", newPath)
        b.putString("kind", kinds.getOrNull(kindSpinner.selectedItemPosition)?.first ?: wantedKind)
        b.putString("title", title.text.toString())
        b.putString("origin", origin.text.toString())
        b.putString("text", text.text.toString())
        b.putString("date", dateIso)
    }

    fun restoreState(b: Bundle?) {
        if (b == null) return
        photoPath = b.getString("photo")?.takeIf { it !in savedPaths }
        newPath = b.getString("new")
        wantedKind = b.getString("kind") ?: "papel"
        title.setText(b.getString("title") ?: "")
        origin.setText(b.getString("origin") ?: "")
        text.setText(b.getString("text") ?: "")
        dateIso = b.getString("date") ?: ""
        updateDate()
        setKinds()
        // A camera result may have arrived while the process was gone.
        newPath?.let { p -> if (File(p).length() > 0) acceptNew(p) }
        showThumb()
    }

    // ------------------------------------------------------------ camera

    private fun takePhoto() {
        val dir = File(a.cacheDir, PonteFiles.DIR_PHOTOS)
        dir.mkdirs()
        val f = File(dir, "foto-" + System.currentTimeMillis() + ".jpg")
        val uri = PonteFiles.uriFor(PonteFiles.DIR_PHOTOS, f.name)
        val i = Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
            putExtra(MediaStore.EXTRA_OUTPUT, uri)
            addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
            clipData = ClipData.newRawUri("foto", uri)
        }
        newPath = f.absolutePath
        try {
            a.startActivityForResult(i, MainActivity.REQ_PHOTO)
        } catch (_: ActivityNotFoundException) {
            newPath = null
            a.toast("Este aparelho não tem aplicativo de câmera.")
        } catch (e: Throwable) {
            newPath = null
            a.toast(e.message ?: e.toString())
        }
    }

    fun onCamera(ok: Boolean) {
        val p = newPath ?: return
        if (ok && File(p).length() > 0) {
            acceptNew(p)
        } else {
            File(p).delete()
            newPath = null
        }
    }

    private fun acceptNew(p: String) {
        val old = photoPath
        photoPath = p
        newPath = null
        if (old != null && old != p) File(old).delete()
        showThumb()
    }

    private fun showThumb() {
        val p = photoPath ?: return
        Thread {
            val bmp = try { Images.decode(File(p), 600) } catch (_: Throwable) { null }
            a.runOnUiThread {
                if (bmp != null) thumb.setImageBitmap(bmp)
                else progress.text = "Não foi possível mostrar a foto."
            }
        }.start()
    }

    // ------------------------------------------------------------ save

    private fun save() {
        if (saving) return
        val t = title.text.toString().trim()
        if (t.isEmpty()) { title.error = "Informe o título."; return }
        val src = photoPath?.let { File(it) }
        if (src == null || !src.isFile) { a.toast("Tire a foto antes de salvar."); return }
        val fields = JSONObject()
            .put("kind", kinds.getOrNull(kindSpinner.selectedItemPosition)?.first ?: "papel")
            .put("title", t)
            .put("origin", origin.text.toString().trim())
            .put("date", dateIso)
            .put("text", text.text.toString())
            .toString()
        saving = true
        saveBtn.isEnabled = false
        progress.setTextColor(Colors.GRAY)
        progress.text = "Preparando a foto..."
        val out = File(src.parentFile, "envio-" + System.currentTimeMillis() + ".jpg")
        Thread {
            try {
                Images.resizeForUpload(src, out)
                a.runOnUiThread { progress.text = "Guardando o registro..." }
                SyncRunner.background({
                    val r = Api.createPhoto(fields, out.absolutePath)
                    SyncRunner.refreshDocs()
                    r.optLong("document_id")
                }, { id ->
                    // Saved: forget everything first so a recreated screen cannot save it twice.
                    savedPaths.add(src.absolutePath)
                    photoPath = null
                    newPath = null
                    title.setText("")
                    origin.setText("")
                    text.setText("")
                    dateIso = ""
                    saving = false
                    src.delete()
                    out.delete()
                    val live = MainActivity.live
                    if (live != null) {
                        live.requestSync(null)
                        live.showDocument(id)
                    }
                }, { fail(it, out) })
            } catch (e: Throwable) {
                a.runOnUiThread { fail(e.message ?: e.toString(), out) }
            }
        }.start()
    }

    private fun fail(msg: String, out: File) {
        out.delete()
        saving = false
        saveBtn.isEnabled = true
        progress.setTextColor(Colors.RED)
        progress.text = msg
    }

    private companion object {
        /** Photos already stored in the library (process-wide), never to be saved again. */
        val savedPaths: MutableSet<String> = java.util.Collections.synchronizedSet(HashSet())
    }

    override fun onBack(): Boolean {
        // Leaving without saving: the pictures are not needed.
        if (saving) return true
        photoPath?.let { File(it).delete() }
        newPath?.let { File(it).delete() }
        return false
    }
}
