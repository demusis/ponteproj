package io.github.demusis.ponte

import android.app.AlertDialog
import android.graphics.Color
import android.text.InputType
import android.view.View
import android.widget.ArrayAdapter
import android.widget.CheckBox
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject

/** Passwords typed for this session (not remembered on the device). Memory only. */
internal object MailSession {
    val passwords = HashMap<String, String>()
}

/** E-mail import: accounts, account form, and the browse/import flow. */
internal class MailScreen(private val a: MainActivity) : Screen {
    private val ui = a.ui
    private val scroll = ScrollView(a)
    private val box = ui.vbox(12)

    private var mode = 0 // 0 accounts, 1 form, 2 browse
    private var busy = false
    private var info: TextView = ui.text("", 13f, Colors.GRAY)

    // browse state
    private var account: JSONObject? = null
    private var folders: List<Pair<String, String>> = emptyList()
    private var messages: List<JSONObject> = emptyList()
    private val checked = HashSet<Long>()
    private var folderSpinner: Spinner? = null
    private var filterSpinner: Spinner? = null
    private var listBox = ui.vbox()
    private var resultBox = ui.vbox()
    private var importBtn = ui.button("") {}
    private var listedFolder = ""

    override val view: View get() = scroll

    init {
        scroll.addView(box)
        showAccounts()
    }

    override fun refresh() {
        if (mode == 0) showAccounts()
    }

    override fun onBack(): Boolean {
        if (mode != 0) {
            showAccounts()
            return true
        }
        return false
    }

    private fun setInfo(msg: String, error: Boolean = false) {
        info.text = msg
        info.setTextColor(if (error) Colors.RED else Colors.GRAY)
    }

    /** Runs a network call on the shared worker with a progress line. */
    private fun <T> net(label: String, work: () -> T, ok: (T) -> Unit) = netOn(false, label, work, ok)

    private fun <T> netServer(label: String, work: () -> T, ok: (T) -> Unit) = netOn(true, label, work, ok)

    private fun <T> netOn(server: Boolean, label: String, work: () -> T, ok: (T) -> Unit) {
        if (busy) return
        busy = true
        setInfo(label)
        val done = { r: T -> busy = false; setInfo(""); ok(r) }
        val fail = { m: String ->
            busy = false
            val acc = account
            if (mode == 2 && acc != null && !acc.optBoolean("has_password")) {
                MailSession.passwords.remove(acc.s("uid")) // maybe wrong: ask again next time
            }
            setInfo(m, true)
        }
        if (server) SyncRunner.backgroundNetwork(work, done, fail) else SyncRunner.background(work, done, fail)
    }

    // ------------------------------------------------------------ accounts

    private fun showAccounts() {
        mode = 0
        box.removeAllViews()
        box.addView(ui.text("E-mail", 20f, bold = true))
        info = ui.text("", 13f, Colors.GRAY)
        box.addView(info, ui.wide())
        box.addView(ui.button("Nova conta") { showForm(null) }, ui.wide())
        setInfo("Carregando...")
        SyncRunner.background({ Api.mailAccounts() }, { list ->
            if (mode != 0) return@background
            setInfo(if (list.isEmpty()) "Nenhuma conta cadastrada." else "")
            for (acc in list) box.addView(accountRow(acc))
        }, { if (mode == 0) setInfo(it, true) })
    }

    private fun accountRow(acc: JSONObject): View {
        val col = ui.vbox()
        col.setPadding(0, ui.dp(8), 0, ui.dp(8))
        col.addView(ui.text(acc.s("name").ifEmpty { acc.s("user") }, 16f, bold = true))
        col.addView(ui.text(acc.s("user") + " · " + acc.s("host"), 12f, Colors.GRAY))
        val row = ui.hbox()
        val open = ui.button("Abrir") { showBrowse(acc) }
        val edit = ui.button("Editar") { showForm(acc) }
        val del = ui.button("Excluir") { confirmDelete(acc) }
        for (b in listOf(open, edit, del)) row.addView(b, ui.weighted())
        col.addView(row, ui.wide())
        return col
    }

    private fun confirmDelete(acc: JSONObject) {
        AlertDialog.Builder(a)
            .setTitle("Excluir a conta?")
            .setMessage(acc.s("name").ifEmpty { acc.s("user") } + "\nOs documentos já importados continuam no acervo.")
            .setPositiveButton("Excluir") { _, _ ->
                val uid = acc.s("uid")
                net("Excluindo...", { Api.deleteMailAccount(uid) }) {
                    MailSession.passwords.remove(uid)
                    showAccounts()
                }
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    // ------------------------------------------------------------ form

    private fun showForm(acc: JSONObject?) {
        mode = 1
        box.removeAllViews()
        box.addView(ui.text(if (acc == null) "Nova conta" else "Editar conta", 20f, bold = true))
        info = ui.text("", 13f, Colors.GRAY)
        box.addView(info, ui.wide())
        val name = ui.edit("Nome", acc?.s("name") ?: "")
        val host = ui.edit("Servidor", acc?.s("host") ?: "imap.gmail.com")
        val port = ui.edit("Porta", (acc?.optInt("port", 993) ?: 993).toString()).apply {
            inputType = InputType.TYPE_CLASS_NUMBER
        }
        val user = ui.edit("Usuário (e-mail)", acc?.s("user") ?: "").apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
        }
        val pw = ui.edit(
            if (acc != null && acc.optBoolean("has_password")) "Senha de app (vazio mantém a guardada)"
            else "Senha de app"
        ).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            isSaveEnabled = false
        }
        val remember = CheckBox(a).apply {
            text = "Lembrar a senha neste aparelho"
            isChecked = acc?.optBoolean("remember") ?: false
        }
        for (v in listOf(name, host, port, user, pw, remember)) box.addView(v, ui.wide())
        box.addView(
            ui.text(
                "Gmail: use uma senha de app (Conta Google ▸ Segurança ▸ Verificação em duas etapas ▸ Senhas de app).",
                12f, Colors.GRAY
            ), ui.wide()
        )
        box.addView(ui.button("Salvar") {
            val u = user.text.toString().trim()
            val h = host.text.toString().trim()
            if (u.isEmpty() || h.isEmpty()) {
                setInfo("Informe o servidor e o usuário.", true)
                return@button
            }
            val password = pw.text.toString()
            val json = JSONObject()
                .put("uid", acc?.s("uid") ?: "")
                .put("name", name.text.toString().trim().ifEmpty { u })
                .put("host", h)
                .put("port", port.text.toString().toIntOrNull() ?: 993)
                .put("user", u)
                .put("password", password)
                .put("remember", remember.isChecked)
                .put("folder", acc?.s("folder", "INBOX") ?: "INBOX")
                .toString()
            net("Salvando...", { Api.saveMailAccount(json) }) { uid ->
                if (password.isNotEmpty() && !remember.isChecked && uid.isNotEmpty()) {
                    MailSession.passwords[uid] = password
                }
                showAccounts()
            }
        }, ui.wide())
        box.addView(ui.button("Cancelar") { showAccounts() }, ui.wide())
    }

    // ------------------------------------------------------------ browse / import

    /** Password for the account: remembered (""), kept for the session, or asked now. */
    private fun withPassword(acc: JSONObject, then: (String) -> Unit) {
        if (acc.optBoolean("has_password")) { then(""); return }
        val uid = acc.s("uid")
        MailSession.passwords[uid]?.let { then(it); return }
        val input = ui.edit("Senha de app").apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }
        AlertDialog.Builder(a)
            .setTitle("Senha da conta")
            .setMessage(acc.s("user"))
            .setView(input)
            .setPositiveButton("OK") { _, _ ->
                val p = input.text.toString()
                if (p.isNotEmpty()) {
                    MailSession.passwords[uid] = p
                    then(p)
                }
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    private fun pwFor(acc: JSONObject) =
        if (acc.optBoolean("has_password")) "" else (MailSession.passwords[acc.s("uid")] ?: "")

    private fun showBrowse(acc: JSONObject) {
        withPassword(acc) { buildBrowse(acc) }
    }

    private fun buildBrowse(acc: JSONObject) {
        mode = 2
        account = acc
        folders = emptyList()
        messages = emptyList()
        checked.clear()
        box.removeAllViews()
        box.addView(ui.text(acc.s("name").ifEmpty { acc.s("user") }, 20f, bold = true))
        box.addView(ui.text(acc.s("user"), 12f, Colors.GRAY))
        info = ui.text("", 13f, Colors.GRAY)
        box.addView(info, ui.wide())
        box.addView(ui.button("Listar pastas") { listFolders(acc) }, ui.wide())

        folderSpinner = Spinner(a)
        filterSpinner = Spinner(a).apply {
            adapter = ArrayAdapter(
                a, android.R.layout.simple_spinner_dropdown_item,
                listOf("Todas", "Com estrela", "Não lidas")
            )
        }
        box.addView(folderSpinner, ui.wide())
        box.addView(filterSpinner, ui.wide())
        box.addView(ui.button("Listar mensagens") { listMessages(acc) }, ui.wide())

        listBox = ui.vbox()
        importBtn = ui.button("Importar 0 marcadas") { importChecked(acc) }
        importBtn.visibility = View.GONE
        resultBox = ui.vbox()
        box.addView(listBox, ui.wide())
        box.addView(importBtn, ui.wide())
        box.addView(resultBox, ui.wide())
        box.addView(ui.button("Voltar às contas") { showAccounts() }, ui.wide())
    }

    private fun listFolders(acc: JSONObject) {
        val pw = pwFor(acc)
        netServer(SERVER_MSG, { Api.mailFolders(acc.s("uid"), pw) }) { list ->
            folders = list.map { it.s("raw") to it.s("name").ifEmpty { it.s("raw") } }
            val sp = folderSpinner ?: return@netServer
            sp.adapter = ArrayAdapter(a, android.R.layout.simple_spinner_dropdown_item, folders.map { it.second })
            val want = acc.s("folder", "INBOX")
            val i = folders.indexOfFirst { it.first == want || it.second == want }
            if (i >= 0) sp.setSelection(i)
            if (folders.isEmpty()) setInfo("Nenhuma pasta encontrada.")
        }
    }

    private fun listMessages(acc: JSONObject) {
        val idx = folderSpinner?.selectedItemPosition ?: -1
        val folder = folders.getOrNull(idx)?.first
        if (folder == null) {
            setInfo("Toque em \"Listar pastas\" e escolha uma pasta.", true)
            return
        }
        val filter = when (filterSpinner?.selectedItemPosition) { 1 -> "estrela"; 2 -> "nao_lidas"; else -> "todas" }
        val pw = pwFor(acc)
        netServer(SERVER_MSG, { Api.mailMessages(acc.s("uid"), pw, folder, filter) }) { list ->
            listedFolder = folder
            messages = list
            checked.clear()
            resultBox.removeAllViews()
            renderMessages()
            if (list.isEmpty()) setInfo("Nenhuma mensagem.")
        }
    }

    private fun renderMessages() {
        listBox.removeAllViews()
        for (m in messages) {
            val uid = m.optLong("uid")
            val imported = m.optBoolean("imported")
            val label = (if (m.optBoolean("flagged")) "★ " else "") + m.s("date") + "\n" +
                m.s("from") + "\n" + m.s("subject").ifEmpty { "(sem assunto)" } +
                (if (imported) "\njá no acervo" else "")
            val cb = CheckBox(a).apply {
                text = label
                textSize = 13f
                isChecked = uid in checked
                if (imported) setTextColor(Color.GRAY)
                setOnCheckedChangeListener { _, on ->
                    if (on) checked.add(uid) else checked.remove(uid)
                    updateImportButton()
                }
            }
            listBox.addView(cb, ui.wide())
        }
        updateImportButton()
    }

    private fun updateImportButton() {
        importBtn.text = "Importar " + checked.size + " marcadas"
        importBtn.visibility = if (messages.isEmpty()) View.GONE else View.VISIBLE
        importBtn.isEnabled = checked.isNotEmpty()
    }

    private fun importChecked(acc: JSONObject) {
        if (checked.isEmpty()) return
        if (!a.networkAllowed()) {
            setInfo("Sem Wi-Fi: importação adiada. Para usar dados móveis, ative em Ajustes.")
            return
        }
        val pw = pwFor(acc)
        val folder = listedFolder
        val ids = checked.toList()
        val arr = JSONArray().also { j -> ids.forEach { j.put(it) } }.toString()
        netServer(SERVER_MSG, {
            val r = Api.importMail(acc.s("uid"), pw, folder, arr)
            SyncRunner.refreshDocs()
            r
        }) { results ->
            resultBox.removeAllViews()
            resultBox.addView(ui.heading("Resultado"))
            val done = HashSet<Long>()
            for (r in results) {
                val ok = r.optBoolean("ok")
                val subject = r.s("subject").ifEmpty { "(sem assunto)" }
                val line = if (ok) {
                    done.add(r.optLong("uid"))
                    "✓ $subject — " +
                        (if (r.optBoolean("already")) "já estava no acervo" else r.optInt("attachments").toString() + " anexo(s)")
                } else {
                    "✗ $subject: " + r.s("error", "erro")
                }
                resultBox.addView(ui.text(line, 13f, if (ok) Colors.GREEN else Colors.RED), ui.wide())
            }
            messages = messages.map {
                if (it.optLong("uid") in done) JSONObject(it.toString()).put("imported", true) else it
            }
            checked.clear()
            renderMessages()
            (MainActivity.live ?: a).requestSync(null)
        }
    }

    private companion object {
        const val SERVER_MSG = "Conversando com o servidor de e-mail…"
    }
}
