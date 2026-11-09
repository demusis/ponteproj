package io.github.demusis.ponte

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * The colors of Ponte de bolso. Screens use these names (through [Ponte.colors]) and never
 * hard-coded colors, so the light and the dark theme stay in step.
 */
internal data class PonteColors(
    val dark: Boolean,
    /** Page background. */
    val ground: Color,
    /** Cards, bars, fields. */
    val surface: Color,
    /** Main text. */
    val text: Color,
    /** Second-line text inside cards (a little stronger than [muted]). */
    val secondary: Color,
    /** Supporting text: dates, captions, placeholders. */
    val muted: Color,
    /** Brand color: main buttons, links, active items. */
    val accent: Color,
    val onAccent: Color,
    /** Soft brand background: type tags, active tab pill, file icons. */
    val accentContainer: Color,
    val onAccentContainer: Color,
    /** Late / failed. */
    val error: Color,
    val errorContainer: Color,
    /** Due today / waiting. */
    val warning: Color,
    val warningContainer: Color,
    /** Done / synchronized. */
    val success: Color,
    val successContainer: Color,
    /** Neutral chip and icon background. */
    val chip: Color,
    /** Field and outlined-button borders. */
    val outline: Color,
    /** Hairlines inside cards. */
    val divider: Color,
    /** Border above the bottom bar and footers. */
    val barBorder: Color,
    /** Placeholder behind a photo. */
    val photoGround: Color,
    /** The floating "N marcadas · Importar" bar. */
    val contrastBar: Color,
    val onContrastBar: Color
)

internal val LightPonteColors = PonteColors(
    dark = false,
    ground = Color(0xFFF4F6F7),
    surface = Color(0xFFFFFFFF),
    text = Color(0xFF17212B),
    secondary = Color(0xFF3B4650),
    muted = Color(0xFF5B6772),
    accent = Color(0xFF185461),
    onAccent = Color(0xFFFFFFFF),
    accentContainer = Color(0xFFDCEBEE),
    onAccentContainer = Color(0xFF185461),
    error = Color(0xFFA8282D),
    errorContainer = Color(0xFFFCE8E9),
    warning = Color(0xFF8A4F0E),
    warningContainer = Color(0xFFFEF3E0),
    success = Color(0xFF147648),
    successContainer = Color(0xFFDFF3E8),
    chip = Color(0xFFE8ECEF),
    outline = Color(0xFFA9B3BC),
    divider = Color(0xFFE1E6EA),
    barBorder = Color(0xFFD5DBE0),
    photoGround = Color(0xFFDDE3E7),
    contrastBar = Color(0xFF17212B),
    onContrastBar = Color(0xFFFFFFFF)
)

internal val DarkPonteColors = PonteColors(
    dark = true,
    ground = Color(0xFF0F1518),
    surface = Color(0xFF1B2327),
    text = Color(0xFFE6EBEE),
    secondary = Color(0xFFC2CBD1),
    muted = Color(0xFF9AA6AF),
    accent = Color(0xFF7CC4D2),
    onAccent = Color(0xFF06323B),
    accentContainer = Color(0xFF17424C),
    onAccentContainer = Color(0xFFCDEAF0),
    error = Color(0xFFFFB3B5),
    errorContainer = Color(0xFF5C1A1E),
    warning = Color(0xFFFFC27A),
    warningContainer = Color(0xFF553308),
    success = Color(0xFF6FD3A0),
    successContainer = Color(0xFF123F2A),
    chip = Color(0xFF2B353B),
    outline = Color(0xFF5A666F),
    divider = Color(0xFF2B353B),
    barBorder = Color(0xFF2B353B),
    photoGround = Color(0xFF263036),
    contrastBar = Color(0xFFE6EBEE),
    onContrastBar = Color(0xFF17212B)
)

internal val LocalPonteColors = staticCompositionLocalOf { LightPonteColors }

/** `Ponte.colors.accent`, like `MaterialTheme.colorScheme`. */
internal object Ponte {
    val colors: PonteColors
        @Composable @ReadOnlyComposable get() = LocalPonteColors.current
}

private fun scheme(c: PonteColors) = (if (c.dark) darkColorScheme() else lightColorScheme()).copy(
    primary = c.accent,
    onPrimary = c.onAccent,
    primaryContainer = c.accentContainer,
    onPrimaryContainer = c.onAccentContainer,
    inversePrimary = if (c.dark) Color(0xFF185461) else Color(0xFF7CC4D2),
    secondary = c.accent,
    onSecondary = c.onAccent,
    secondaryContainer = c.accentContainer,
    onSecondaryContainer = c.onAccentContainer,
    tertiary = c.accent,
    onTertiary = c.onAccent,
    tertiaryContainer = c.accentContainer,
    onTertiaryContainer = c.onAccentContainer,
    background = c.ground,
    onBackground = c.text,
    surface = c.surface,
    onSurface = c.text,
    surfaceVariant = c.chip,
    onSurfaceVariant = c.muted,
    surfaceTint = c.accent,
    inverseSurface = c.contrastBar,
    inverseOnSurface = c.onContrastBar,
    error = c.error,
    onError = if (c.dark) Color(0xFF5C1A1E) else Color.White,
    errorContainer = c.errorContainer,
    onErrorContainer = c.error,
    outline = c.outline,
    outlineVariant = c.divider,
    scrim = Color.Black,
    surfaceBright = c.surface,
    surfaceDim = c.ground,
    surfaceContainerLowest = c.surface,
    surfaceContainerLow = c.surface,
    surfaceContainer = c.surface,
    surfaceContainerHigh = if (c.dark) Color(0xFF222B30) else c.surface,
    surfaceContainerHighest = c.chip
)

private val PonteTypography = Typography().let { t ->
    t.copy(
        // Screen title in the top bar.
        titleLarge = t.titleLarge.copy(fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.Medium),
        // Document title on the ficha.
        headlineSmall = t.headlineSmall.copy(fontSize = 22.sp, lineHeight = 28.sp, fontWeight = FontWeight.SemiBold),
        // Card headings.
        titleMedium = t.titleMedium.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold),
        // Card titles (task, document, message subject).
        titleSmall = t.titleSmall.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Medium),
        bodyLarge = t.bodyLarge.copy(fontSize = 16.sp, lineHeight = 24.sp, letterSpacing = 0.sp),
        // Running text inside cards.
        bodyMedium = t.bodyMedium.copy(fontSize = 15.sp, lineHeight = 22.sp, letterSpacing = 0.sp),
        // Second lines and captions.
        bodySmall = t.bodySmall.copy(fontSize = 13.sp, lineHeight = 18.sp, letterSpacing = 0.sp),
        labelLarge = t.labelLarge.copy(fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold),
        // Chips and tags.
        labelMedium = t.labelMedium.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.sp),
        // Section labels ("ATRASADAS · 2").
        labelSmall = t.labelSmall.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.6.sp)
    )
}

private val PonteShapes = Shapes(
    extraSmall = RoundedCornerShape(6.dp),
    small = RoundedCornerShape(10.dp),
    medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(24.dp)
)

/** Field label above an input ("Título"). */
internal val FieldLabelStyle = TextStyle(fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium)

@Composable
internal fun PonteTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val colors = if (dark) DarkPonteColors else LightPonteColors
    CompositionLocalProvider(LocalPonteColors provides colors) {
        MaterialTheme(
            colorScheme = scheme(colors),
            typography = PonteTypography,
            shapes = PonteShapes,
            content = content
        )
    }
}
