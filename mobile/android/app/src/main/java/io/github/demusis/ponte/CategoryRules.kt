package io.github.demusis.ponte

import org.json.JSONObject
import java.text.Normalizer

/**
 * A category of the library (one list for tasks and documents). [color] is an index into the palette
 * of [CategoryRules]; the same ten hues as the Ponte on the computer. Renaming, recolouring, archiving
 * and deleting are done on the computer only.
 */
internal data class Category(
    val uid: String,
    val name: String,
    val color: Int,
    val archived: Boolean = false
)

internal fun JSONObject.toCategory() = Category(
    uid = s("uid"), name = s("name"), color = optInt("color"), archived = optBoolean("archived")
)

/** The colours of a category chip, as ARGB: text, background, and the dot. */
internal data class ChipColors(val text: Int, val background: Int, val dot: Int)

/** Pure rules of the categories (colour, filter, limit, creation), tested without Android. */
internal object CategoryRules {
    /** What the core accepts. */
    const val MAX_PER_ITEM = 10
    const val MAX_NAME = 40
    const val MAX_CATEGORIES = 100
    const val PALETTE_SIZE = 10

    /** The filter's marker for "items without any category" (never a real uid). */
    const val NONE = "sem-categoria"

    /** Base hues, the same as `HUES` of the computer's `category_ui.rs`. */
    private val HUES = listOf(
        intArrayOf(30, 150, 160), // teal
        intArrayOf(50, 110, 210), // blue
        intArrayOf(110, 80, 200), // violet
        intArrayOf(190, 70, 170), // magenta
        intArrayOf(210, 60, 70), // red
        intArrayOf(220, 100, 30), // orange
        intArrayOf(200, 150, 10), // amber
        intArrayOf(110, 150, 30), // olive
        intArrayOf(40, 160, 90), // green
        intArrayOf(100, 115, 125) // slate
    )

    /** The dark theme's surface, which the tints are mixed into (as on the computer). */
    private val DARK_SURFACE = intArrayOf(26, 36, 40)

    private fun mix(a: IntArray, b: IntArray, t: Double): Int {
        fun m(x: Int, y: Int) = Math.round(x * (1.0 - t) + y * t).toInt().coerceIn(0, 255)
        return argb(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]))
    }

    private fun argb(r: Int, g: Int, b: Int) = (0xFF shl 24) or (r shl 16) or (g shl 8) or b

    /** Colours of palette entry [index] (a bad index is held to the palette). */
    fun colors(index: Int, dark: Boolean): ChipColors {
        val hue = HUES[index.coerceIn(0, PALETTE_SIZE - 1)]
        val dot = argb(hue[0], hue[1], hue[2])
        return if (dark) {
            ChipColors(mix(intArrayOf(255, 255, 255), hue, 0.25), mix(DARK_SURFACE, hue, 0.22), dot)
        } else {
            ChipColors(mix(intArrayOf(0, 0, 0), hue, 0.5), mix(intArrayOf(255, 255, 255), hue, 0.16), dot)
        }
    }

    /** Case and accents do not tell two names apart. */
    fun nameKey(name: String): String =
        Normalizer.normalize(name.trim(), Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "").lowercase()

    /** An item matches when it has ANY of the chosen categories, or none and [NONE] is chosen. */
    fun matches(itemUids: List<String>, chosen: Set<String>): Boolean = when {
        chosen.isEmpty() -> true
        itemUids.isEmpty() -> NONE in chosen
        else -> itemUids.any { it in chosen }
    }

    /** Adds [uid] when absent (until the item limit), removes it when present. */
    fun toggle(selected: List<String>, uid: String): List<String> = when {
        uid in selected -> selected - uid
        selected.size >= MAX_PER_ITEM -> selected
        else -> selected + uid
    }

    fun isFull(selected: List<String>) = selected.size >= MAX_PER_ITEM

    /** The same toggle for the filter (no item limit; [NONE] is a choice like the others). */
    fun toggleFilter(chosen: Set<String>, key: String): Set<String> =
        if (key in chosen) chosen - key else chosen + key

    /**
     * What the picker lists for the typed [query]: active categories whose name contains it, then the
     * archived ones that are already on the item (so they can be taken off; they cannot be added).
     */
    fun offered(all: List<Category>, selected: List<String>, query: String): List<Category> {
        val q = nameKey(query)
        val hit = { c: Category -> q.isEmpty() || nameKey(c.name).contains(q) }
        val active = all.filter { !it.archived && hit(it) }
        val archivedOnItem = all.filter { it.archived && it.uid in selected && hit(it) }
        return active + archivedOnItem
    }

    /** What the filter lists: the active categories (an archived one only while it is chosen). */
    fun filterOptions(all: List<Category>, chosen: Set<String>): List<Category> =
        all.filter { !it.archived || it.uid in chosen }

    /** "Criar" is offered only for a usable name that no category (archived ones too) already has. */
    fun canCreate(query: String, all: List<Category>): Boolean {
        val name = query.trim()
        return name.isNotEmpty() && name.length <= MAX_NAME && all.size < MAX_CATEGORIES &&
            all.none { nameKey(it.name) == nameKey(name) }
    }

    /** The first palette entry no category uses; when all are used, the least used (the first of them). */
    fun nextColor(all: List<Category>): Int {
        val uses = IntArray(PALETTE_SIZE)
        for (c in all) uses[c.color.coerceIn(0, PALETTE_SIZE - 1)]++
        val least = uses.min()
        return uses.indexOfFirst { it == least }
    }

    /** The categories of an item that the library knows (an uid not yet synced shows nothing). */
    fun resolve(uids: List<String>, all: List<Category>): List<Category> {
        val byUid = all.associateBy { it.uid }
        return uids.mapNotNull { byUid[it] }
    }

    /** What a compact row shows: the first [max] chips and how many were left out. */
    fun compact(chips: List<Category>, max: Int = 2): Pair<List<Category>, Int> =
        chips.take(max) to (chips.size - max).coerceAtLeast(0)

    /** The summary of an active filter for its button: "Categorias", or "Categorias · 2". */
    fun filterLabel(chosen: Set<String>) = if (chosen.isEmpty()) "Categorias" else "Categorias · ${chosen.size}"

    /** The filters keep only what still exists (a category deleted on the computer). */
    fun pruneFilter(chosen: Set<String>, all: List<Category>): Set<String> =
        chosen.filter { it == NONE || all.any { c -> c.uid == it } }.toSet()
}
