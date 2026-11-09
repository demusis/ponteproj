package io.github.demusis.ponte

import android.graphics.Color
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.ViewGroup
import android.widget.BaseAdapter
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView

/** Searchable list of documents. */
internal class DocumentsScreen(private val a: MainActivity) : Screen {
    private val ui = a.ui
    private var shown: List<Doc> = emptyList()
    private val adapter = DocAdapter()
    private val search = ui.edit("Buscar por número ou título")
    private val empty = ui.text("", 13f, Colors.GRAY)
    override val view = ui.vbox()

    init {
        search.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun afterTextChanged(s: Editable?) = filter()
        })
        view.addView(search, ui.wide())
        view.addView(ui.button("+ Registro por foto") { a.showPhoto() }, ui.wide())
        view.addView(empty, ui.wide())
        val list = ListView(a).apply {
            adapter = this@DocumentsScreen.adapter
            setOnItemClickListener { _, _, pos, _ ->
                shown.getOrNull(pos)?.let { a.showDocument(it.id) }
            }
        }
        view.addView(list, ui.fill())
        filter()
    }

    override fun refresh() = filter()

    private fun filter() {
        val q = search.text.toString().trim().lowercase()
        val all = SyncRunner.docs
        shown = if (q.isEmpty()) all else all.filter {
            it.number.lowercase().contains(q) || it.title.lowercase().contains(q)
        }
        empty.text = if (shown.isEmpty()) "Nenhum documento." else ""
        empty.visibility = if (shown.isEmpty()) View.VISIBLE else View.GONE
        adapter.notifyDataSetChanged()
    }

    private inner class DocAdapter : BaseAdapter() {
        override fun getCount() = shown.size
        override fun getItem(position: Int) = shown[position]
        override fun getItemId(position: Int) = shown[position].id

        // Two-line row: "number — title" and a smaller note preview.
        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val d = shown[position]
            val row = (convertView as? LinearLayout) ?: LinearLayout(a).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(ui.dp(4), ui.dp(8), ui.dp(4), ui.dp(8))
                addView(TextView(context).apply { textSize = 16f })
                addView(TextView(context).apply {
                    textSize = 12f
                    setTextColor(Color.GRAY)
                    maxLines = 2
                })
            }
            (row.getChildAt(0) as TextView).text = d.number + " — " + d.title
            (row.getChildAt(1) as TextView).text =
                if (d.noteError != null) "(nota indisponível: " + d.noteError + ")" else d.note
            return row
        }
    }
}
