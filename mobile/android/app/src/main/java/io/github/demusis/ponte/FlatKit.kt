package io.github.demusis.ponte

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// The flat building blocks of the pushed screens (ficha, forms, Ajustes...): a section label, rows of facts and of
// links, and text buttons, all on the page margin of [ROW_MARGIN]. No card, no outline, no filled chip.

/** A section label on the page margin ("DADOS"); [action] is a small control at the end of the same line. */
@Composable
internal fun SectionHeader(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Ponte.colors.muted,
    action: (@Composable () -> Unit)? = null
) {
    Row(
        modifier.fillMaxWidth().padding(start = ROW_MARGIN, end = if (action == null) ROW_MARGIN else 4.dp, top = 20.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text.uppercase(), Modifier.weight(1f), style = MaterialTheme.typography.labelSmall, fontSize = 13.sp, color = color
        )
        action?.invoke()
    }
}

/** A flat section: its label, then [content] (rows or text with their own margin), then a hairline. */
@Composable
internal fun FlatSection(
    title: String?,
    modifier: Modifier = Modifier,
    color: Color = Ponte.colors.muted,
    divider: Boolean = true,
    action: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit
) {
    Column(modifier.fillMaxWidth()) {
        if (title != null) SectionHeader(title, color = color, action = action)
        content()
        if (divider) RowDivider(0.dp)
    }
}

/** Free content of a section (text, fields) with the page margin on both sides. */
@Composable
internal fun SectionBody(modifier: Modifier = Modifier, spacing: Dp = 12.dp, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = ROW_MARGIN, vertical = 6.dp),
        verticalArrangement = Arrangement.spacedBy(spacing), content = content
    )
}

/** A fact: its small label over its value. */
@Composable
internal fun FactRow(label: String, value: String, modifier: Modifier = Modifier, muted: Boolean = false) {
    val c = Ponte.colors
    Column(modifier.fillMaxWidth().padding(horizontal = ROW_MARGIN, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(label, style = FieldLabelStyle, color = c.secondary)
        Text(value, fontSize = 16.sp, lineHeight = 22.sp, color = if (muted) c.muted else c.text)
    }
}

/** The title block of a ficha: [overline] (kind and date as plain text), a large title, and lines under it. */
@Composable
internal fun FichaHeader(
    title: String,
    modifier: Modifier = Modifier,
    overline: String = "",
    lines: List<String> = emptyList(),
    extra: (@Composable ColumnScope.() -> Unit)? = null
) {
    val c = Ponte.colors
    Column(modifier.fillMaxWidth().padding(horizontal = ROW_MARGIN, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (overline.isNotBlank()) {
            Text(overline, style = MaterialTheme.typography.labelMedium, color = c.accent, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (title.isNotBlank()) {
            Text(title, fontSize = 22.sp, lineHeight = 29.sp, fontWeight = FontWeight.SemiBold, color = c.text)
        }
        for (line in lines) if (line.isNotBlank()) Text(line, style = MaterialTheme.typography.bodyLarge, color = c.secondary)
        extra?.invoke(this)
    }
}

/** A row that leads to another screen: icon, a small label over a text, and a chevron. The press covers the row. */
@Composable
internal fun LinkRow(
    icon: ImageVector?,
    label: String,
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    secondary: String = "",
    divider: Boolean = true
) {
    val c = Ponte.colors
    FlatRow(onClick, modifier, if (icon != null) 54.dp else ROW_MARGIN, divider) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (icon != null) {
                Icon(icon, contentDescription = null, tint = c.secondary, modifier = Modifier.size(22.dp))
                Spacer(Modifier.width(16.dp))
            }
            Column(Modifier.weight(1f)) {
                if (label.isNotBlank()) Text(label, style = FieldLabelStyle, color = c.secondary)
                Text(
                    text, fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium, color = c.text,
                    maxLines = 3, overflow = TextOverflow.Ellipsis
                )
                if (secondary.isNotBlank()) {
                    Text(secondary, style = MaterialTheme.typography.bodySmall, color = c.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, contentDescription = null, tint = c.muted)
        }
    }
}

/**
 * A text button: the way of the secondary actions. Left aligned on the page margin (or [centered]); 48 dp tall.
 * [danger] colours it as a destructive action. Rarely used ones belong in the "⋮" menu instead.
 */
@Composable
internal fun TextAction(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    danger: Boolean = false,
    centered: Boolean = false,
    icon: ImageVector? = null
) {
    val c = Ponte.colors
    val color = when {
        !enabled -> c.muted
        danger -> c.error
        else -> c.accent
    }
    Row(
        modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 12.dp),
        horizontalArrangement = if (centered) Arrangement.Center else Arrangement.Start,
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
        }
        Text(text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = color, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

/** A text action on the page margin of a flat section (aligned with the text of the rows). */
@Composable
internal fun SectionAction(
    text: String, onClick: () -> Unit, enabled: Boolean = true, danger: Boolean = false, icon: ImageVector? = null
) {
    TextAction(text, onClick, Modifier.padding(horizontal = 4.dp), enabled, danger, icon = icon)
}

/** A group of controls of a settings-like screen: no box, closed by a hairline. Its content brings its own gutters. */
@Composable
internal fun FlatBlock(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        content()
        RowDivider(0.dp)
    }
}
