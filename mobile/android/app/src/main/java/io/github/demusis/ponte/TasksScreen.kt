package io.github.demusis.ponte

import android.app.AlertDialog
import android.graphics.Color
import android.view.View
import android.widget.ArrayAdapter
import android.widget.ScrollView
import android.widget.Spinner
import org.json.JSONObject

/** Tasks grouped by due date. */
internal class TasksScreen(private val a: MainActivity) : Screen {
    private val ui = a.ui
    private val scroll = ScrollView(a)
    private val box = ui.vbox(12)
    private var doneOpen = false

    override val view: View get() = scroll

    init {
        scroll.addView(box)
        box.addView(ui.text("Carregando...", color = Colors.GRAY))
        refresh()
    }

    override fun refresh() {
        SyncRunner.background({ Api.tasks() }, { build(it) }, {
            box.removeAllViews()
            box.addView(ui.text(it, color = Colors.RED))
        })
    }

    private fun build(all: List<JSONObject>) {
        box.removeAllViews()
        val open = all.filter { it.s("status") != "concluida" }
        val done = all.filter { it.s("status") == "concluida" }.sortedByDescending { it.optLong("id") }.take(30)
        val late = open.filter { it.optBoolean("overdue") }
        val today = open.filter { !it.optBoolean("overdue") && it.optBoolean("due_today") }
        val rest = open.filter { !it.optBoolean("overdue") && !it.optBoolean("due_today") }
        val next = rest.filter { it.s("due_date").isNotEmpty() }.sortedBy { it.s("due_date") }
        val none = rest.filter { it.s("due_date").isEmpty() }

        section("Atrasadas", late, Colors.RED)
        section("Para hoje", today, Color.BLACK)
        section("Próximas", next, Color.BLACK)
        section("Sem prazo", none, Color.BLACK)

        val header = ui.button((if (doneOpen) "▾ " else "▸ ") + "Concluídas (" + done.size + ")") {}
        header.isAllCaps = false
        header.setOnClickListener { doneOpen = !doneOpen; build(all) }
        box.addView(header, ui.wide())
        if (doneOpen) for (t in done) box.addView(row(t, true))
        if (open.isEmpty()) box.addView(ui.text("Nenhuma tarefa em aberto.", color = Colors.GRAY))
    }

    private fun section(title: String, list: List<JSONObject>, color: Int) {
        if (list.isEmpty()) return
        box.addView(ui.text("$title (${list.size})", 17f, color, bold = true).apply {
            setPadding(0, ui.dp(12), 0, ui.dp(4))
        })
        for (t in list) box.addView(row(t, false))
    }

    private fun row(t: JSONObject, done: Boolean): View {
        val col = ui.vbox()
        col.setPadding(ui.dp(4), ui.dp(8), ui.dp(4), ui.dp(8))
        col.addView(ui.text(t.s("title"), 16f, if (done) Colors.GRAY else Color.BLACK, bold = !done))
        val parts = listOf(t.s("action_label"), t.s("document_number"), t.s("due_label")).filter { it.isNotEmpty() }
        val late = t.optBoolean("overdue") && !done
        col.addView(ui.text(parts.joinToString(" · "), 12f, if (late) Colors.RED else Colors.GRAY))
        col.setOnClickListener { actions(t, done) }
        return col
    }

    private fun actions(t: JSONObject, done: Boolean) {
        val labels = arrayOf(if (done) "Reabrir" else "Concluir", "Abrir documento")
        AlertDialog.Builder(a)
            .setTitle(t.s("title"))
            .setItems(labels) { _, which ->
                if (which == 0) a.changeTask(t.optLong("id"), !done)
                else a.showDocument(t.optLong("document_id"))
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }
}

/** Dialog to add a task to a document: title (required), action, optional due date. */
internal fun showNewTaskDialog(a: MainActivity, docId: Long) {
    SyncRunner.background({ Api.choices() }, { ch ->
        val ui = a.ui
        val actions = ch.optJSONArray("actions").objs()
        val keys = actions.map { it.s("key") }
        val labels = actions.map { it.s("label") }
        val box = ui.vbox(16)
        val title = ui.edit("O que fazer")
        val spinner = Spinner(a).apply {
            adapter = ArrayAdapter(a, android.R.layout.simple_spinner_dropdown_item, labels)
        }
        var due = ""
        val dueBtn = ui.button("Prazo: sem prazo") {}
        dueBtn.isAllCaps = false
        dueBtn.setOnClickListener {
            ui.pickDate(due) { d ->
                due = d
                dueBtn.text = "Prazo: $d"
            }
        }
        val clear = ui.button("Sem prazo") { due = ""; dueBtn.text = "Prazo: sem prazo" }
        clear.isAllCaps = false
        box.addView(title, ui.wide())
        box.addView(ui.text("Ação", 13f, Colors.GRAY), ui.wide())
        box.addView(spinner, ui.wide())
        box.addView(dueBtn, ui.wide())
        box.addView(clear, ui.wide())
        val dlg = AlertDialog.Builder(a)
            .setTitle("Nova tarefa")
            .setView(box)
            .setPositiveButton("Criar", null)
            .setNegativeButton("Cancelar", null)
            .create()
        dlg.show()
        // Own click handler so the dialog stays open while the title is missing.
        dlg.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val text = title.text.toString().trim()
            if (text.isEmpty()) {
                title.error = "Informe o título."
                return@setOnClickListener
            }
            val action = keys.getOrNull(spinner.selectedItemPosition) ?: ""
            dlg.dismiss()
            SyncRunner.background({ Api.createTask(docId, text, action, due) }, {
                a.current?.refresh()
                a.requestSync(null)
            }, { a.status(it, true) })
        }
    }, { a.toast(it) })
}
