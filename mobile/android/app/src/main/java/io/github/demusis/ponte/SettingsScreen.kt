package io.github.demusis.ponte

import android.provider.OpenableColumns
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.View
import android.widget.CheckBox
import android.widget.ScrollView
import android.widget.TextView

/** Settings: Drive file, passphrase options, mobile data, start over. */
internal class SettingsScreen(private val a: MainActivity) : Screen {
    private val ui = a.ui
    private val scroll = ScrollView(a)
    private val box = ui.vbox(12)
    private val fileInfo: TextView = ui.text("Nenhum arquivo escolhido.", 13f, Colors.GRAY)
    private val pass = ui.edit("Frase-senha")
    private val remember = CheckBox(a)
    private var settingBox = false

    override val view: View get() = scroll

    init {
        scroll.addView(box)
        box.addView(ui.text("Ajustes", 20f, bold = true))

        box.addView(ui.heading("Arquivo no Drive"))
        box.addView(ui.button("Escolher o arquivo ponte-sync.ponte no Drive") { a.pickFile() }, ui.wide())
        box.addView(fileInfo, ui.wide())

        box.addView(ui.heading("Frase-senha"))
        pass.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        pass.isSaveEnabled = false // never kept in saved instance state
        pass.setText(SyncRunner.passphrase)
        pass.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, x: Int, y: Int, z: Int) {}
            override fun onTextChanged(s: CharSequence?, x: Int, y: Int, z: Int) {}
            override fun afterTextChanged(s: Editable?) {
                SyncRunner.passphrase = s?.toString() ?: ""
            }
        })
        box.addView(pass, ui.wide())

        if (Secrets.supported()) {
            remember.text = "Lembrar a frase-senha (liberada pela digital ou PIN)"
            setRemember(Secrets.isRemembered(a))
            remember.setOnCheckedChangeListener { _, on -> if (!settingBox) onRememberChanged(on) }
            box.addView(remember, ui.wide())
        }
        box.addView(ui.button("Esquecer frase-senha") { forgetPassphrase() }, ui.wide())

        box.addView(ui.heading("Rede"))
        val mobile = CheckBox(a).apply {
            text = "Sincronizar também nos dados móveis"
            isChecked = a.prefs.getBoolean("mobile_ok", false)
            setOnCheckedChangeListener { _, on -> a.prefs.edit().putBoolean("mobile_ok", on).apply() }
        }
        box.addView(mobile, ui.wide())

        if (SyncRunner.keyBroken) {
            box.addView(ui.heading("Chave do aparelho"))
            box.addView(ui.text("Não foi possível abrir a chave deste aparelho.", color = Colors.RED))
            box.addView(ui.button("Recomeçar o acervo deste aparelho") { a.confirmReset() }, ui.wide())
        }
        refresh()
    }

    private fun setRemember(on: Boolean) {
        settingBox = true
        remember.isChecked = on
        settingBox = false
    }

    private fun onRememberChanged(on: Boolean) {
        if (on) {
            val p = SyncRunner.passphrase
            if (p.isEmpty()) {
                a.toast("Digite a frase-senha primeiro.")
                setRemember(false)
                return
            }
            Secrets.remember(a, p) { ok, msg ->
                if (!ok) setRemember(false)
                a.toast(msg)
            }
        } else {
            Secrets.forget(a)
        }
    }

    private fun forgetPassphrase() {
        SyncRunner.passphrase = ""
        pass.setText("")
        Secrets.forget(a)
        if (Secrets.supported()) setRemember(false)
        a.toast("Frase-senha esquecida.")
    }

    override fun refresh() {
        val uri = a.fileUri ?: return
        fileInfo.text = "Arquivo escolhido."
        val resolver = a.contentResolver
        // The provider may hit the network: query off the UI thread.
        Thread {
            val t = try {
                var s = "Arquivo escolhido."
                resolver.query(uri, null, null, null, null)?.use { c ->
                    if (c.moveToFirst()) {
                        val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val z = c.getColumnIndex(OpenableColumns.SIZE)
                        val name = if (n >= 0) c.getString(n) else "(sem nome)"
                        val size = if (z >= 0 && !c.isNull(z)) c.getLong(z) else -1L
                        s = "Arquivo: " + name + (if (size >= 0) " (" + size + " bytes)" else "")
                    }
                }
                s
            } catch (_: Throwable) {
                "Arquivo escolhido (não foi possível ler os detalhes)."
            }
            a.runOnUiThread { try { fileInfo.text = t } catch (_: Throwable) { } }
        }.start()
    }
}
