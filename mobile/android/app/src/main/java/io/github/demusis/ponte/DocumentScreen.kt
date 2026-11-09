package io.github.demusis.ponte

import android.app.AlertDialog
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.ScrollView
import org.json.JSONObject

/** One document: data, note, analysis summary, tasks, files, attachments and text. */
internal class DocumentScreen(private val a: MainActivity, private val docId: Long) : Screen {
    private val ui = a.ui
    private val scroll = ScrollView(a)
    private val box = ui.vbox(12)
    private var textOpen = false

    override val view: View get() = scroll

    init {
        scroll.addView(box)
        box.addView(ui.text("Carregando...", color = Colors.GRAY))
        refresh()
    }

    override fun refresh() {
        SyncRunner.background({ Api.document(docId) }, { build(it) }, {
            box.removeAllViews()
            box.addView(ui.text(it, color = Colors.RED))
        })
    }

    private fun line(label: String, value: String) {
        if (value.isNotEmpty()) box.addView(ui.text("$label: $value"))
    }

    private fun build(o: JSONObject) {
        box.removeAllViews()
        box.addView(ui.text(o.s("number") + " — " + o.s("title"), 20f, bold = true))
        line("Tipo", o.s("type"))
        line("Data", o.s("date"))
        if (o.optBoolean("email") && o.s("email_from").isNotEmpty()) {
            line("Origem", "De: " + o.s("email_from"))
        } else {
            line("Origem", o.s("origin"))
        }

        // Note
        box.addView(ui.heading("Nota"))
        val note = o.s("note")
        box.addView(ui.text(if (note.isEmpty()) "(sem nota)" else note, color = if (note.isEmpty()) Colors.GRAY else Color.BLACK))
        box.addView(ui.button("Editar nota") { editNote(note) })

        // Analysis
        val an = o.optJSONObject("analysis")
        if (an != null) {
            box.addView(ui.heading("Resumo da análise"))
            box.addView(ui.text(an.s("summary")))
            line("Prioridade", an.s("priority"))
            line("Motivo", an.s("reason"))
            line("Prazo", an.s("deadline"))
            val sug = an.optJSONArray("suggested").objs()
            if (sug.isNotEmpty()) {
                box.addView(ui.text("Ações sugeridas:", bold = true))
                for (s in sug) {
                    val reason = s.s("reason")
                    box.addView(ui.text("• " + s.s("action") + (if (reason.isNotEmpty()) " — $reason" else "")))
                }
            }
        }

        // Tasks
        box.addView(ui.heading("Tarefas"))
        val tasks = o.optJSONArray("tasks").objs()
        if (tasks.isEmpty()) box.addView(ui.text("Nenhuma tarefa.", color = Colors.GRAY))
        for (t in tasks) box.addView(taskRow(t))
        box.addView(ui.button("+ Nova tarefa") { showNewTaskDialog(a, docId) })

        // Files
        val files = o.optJSONArray("files").objs()
        if (files.isNotEmpty()) {
            box.addView(ui.heading("Arquivos"))
            for (f in files) {
                val label = f.s("label", "PDF") + " (" + formatSize(f.optLong("size")) + ")"
                box.addView(ui.button(label) {
                    openExported(a, "pdf", f.optLong("id"), f.optLong("capture_id"))
                }, ui.wide())
            }
        }
        val atts = o.optJSONArray("attachments").objs()
        if (atts.isNotEmpty()) {
            box.addView(ui.heading("Anexos"))
            for (t in atts) {
                val label = t.s("name") + " (" + formatSize(t.optLong("size")) + ")"
                box.addView(ui.button(label) { openExported(a, "attachment", t.optLong("id"), 0L) }, ui.wide())
                val d = t.s("description")
                if (d.isNotEmpty()) box.addView(ui.text(d, 12f, Colors.GRAY))
            }
        }

        // Text (collapsed by default)
        box.addView(ui.heading("Texto do documento"))
        val body = ui.text(o.s("text").ifEmpty { "(sem texto)" }).apply {
            setTextIsSelectable(true)
            visibility = if (textOpen) View.VISIBLE else View.GONE
        }
        val toggle = ui.button(if (textOpen) "Ocultar o texto" else "Mostrar o texto") {}
        toggle.setOnClickListener {
            textOpen = !textOpen
            body.visibility = if (textOpen) View.VISIBLE else View.GONE
            toggle.text = if (textOpen) "Ocultar o texto" else "Mostrar o texto"
        }
        box.addView(toggle, ui.wide())
        box.addView(body, ui.wide())
    }

    private fun taskRow(t: JSONObject): View {
        val done = t.s("status") == "concluida"
        val row = ui.hbox()
        val btn = ui.button(if (done) "☑" else "☐") { confirmToggle(t, done) }
        row.addView(btn)
        val col = ui.vbox()
        col.addView(ui.text(t.s("title"), 15f, bold = !done, color = if (done) Colors.GRAY else Color.BLACK))
        val extra = listOf(t.s("action_label"), t.s("due_label")).filter { it.isNotEmpty() }.joinToString(" · ")
        val overdue = t.optBoolean("overdue") && !done
        if (extra.isNotEmpty()) col.addView(ui.text(extra, 12f, if (overdue) Colors.RED else Colors.GRAY))
        row.addView(col, ui.weighted())
        return row
    }

    private fun confirmToggle(t: JSONObject, done: Boolean) {
        AlertDialog.Builder(a)
            .setTitle(if (done) "Reabrir a tarefa?" else "Concluir a tarefa?")
            .setMessage(t.s("title"))
            .setPositiveButton(if (done) "Reabrir" else "Concluir") { _, _ ->
                a.changeTask(t.optLong("id"), !done)
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    private fun editNote(current: String) {
        val input = ui.edit("Nota", current, multiline = true).apply {
            minLines = 4
            gravity = Gravity.TOP or Gravity.START
        }
        AlertDialog.Builder(a)
            .setTitle("Nota")
            .setView(input)
            .setPositiveButton("Salvar e sincronizar") { _, _ ->
                a.requestSync(Pair(docId, input.text.toString()))
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }
}
