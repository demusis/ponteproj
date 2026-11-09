package io.github.demusis.ponte

import android.app.DatePickerDialog
import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import java.util.Calendar

internal object Colors {
    val RED = Color.RED
    val GREEN = Color.rgb(0x1B, 0x7F, 0x2A)
    val AMBER = Color.rgb(0xB3, 0x6B, 0x00)
    val GRAY = Color.GRAY
}

/** Small helpers to build views in code. */
internal class Ui(private val ctx: Context) {
    fun dp(v: Int) = (v * ctx.resources.displayMetrics.density).toInt()

    fun text(s: CharSequence, size: Float = 14f, color: Int = Color.BLACK, bold: Boolean = false) =
        TextView(ctx).apply {
            text = s
            textSize = size
            setTextColor(color)
            if (bold) setTypeface(typeface, Typeface.BOLD)
        }

    fun heading(s: String) = text(s, 17f, bold = true).apply { setPadding(0, dp(16), 0, dp(4)) }

    fun button(label: String, onClick: () -> Unit) = Button(ctx).apply {
        text = label
        setOnClickListener {
            try {
                onClick()
            } catch (e: Throwable) {
                Toast.makeText(ctx, e.message ?: e.toString(), Toast.LENGTH_LONG).show()
            }
        }
    }

    fun vbox(pad: Int = 0) = LinearLayout(ctx).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(pad), dp(pad), dp(pad), dp(pad))
    }

    fun hbox() = LinearLayout(ctx).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }

    fun edit(hint: String, value: String = "", multiline: Boolean = false) = EditText(ctx).apply {
        this.hint = hint
        setText(value)
        inputType = InputType.TYPE_CLASS_TEXT or
            (if (multiline) InputType.TYPE_TEXT_FLAG_MULTI_LINE else 0)
        if (multiline) {
            minLines = 3
            gravity = Gravity.TOP or Gravity.START
        }
    }

    fun wide() = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT
    )

    fun weighted() = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)

    fun fill() = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)

    /** Opens a date picker; onPick receives AAAA-MM-DD. current may be "" or AAAA-MM-DD. */
    fun pickDate(current: String, onPick: (String) -> Unit) {
        val cal = Calendar.getInstance()
        val p = current.split("-")
        var y = cal.get(Calendar.YEAR)
        var m = cal.get(Calendar.MONTH)
        var d = cal.get(Calendar.DAY_OF_MONTH)
        if (p.size == 3) {
            y = p[0].toIntOrNull() ?: y
            m = (p[1].toIntOrNull() ?: (m + 1)) - 1
            d = p[2].toIntOrNull() ?: d
        }
        DatePickerDialog(ctx, { _, yy, mm, dd ->
            onPick(String.format("%04d-%02d-%02d", yy, mm + 1, dd))
        }, y, m, d).show()
    }
}

internal fun formatSize(n: Long): String = when {
    n >= 1024 * 1024 -> String.format("%.1f MB", n / 1048576.0)
    n >= 1024 -> String.format("%d KB", n / 1024)
    else -> "$n bytes"
}
