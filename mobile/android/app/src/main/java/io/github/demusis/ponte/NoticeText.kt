package io.github.demusis.ponte

/** Adds the counts absent from the core's daily sentence. */
internal object NoticeText {
    fun build(base: String, followUps: Int, risks: Int): String {
        val extra = mutableListOf<String>()
        if (followUps > 0) extra.add(if (followUps == 1) "1 cobrança para hoje" else "$followUps cobranças para hoje")
        if (risks > 0) extra.add(if (risks == 1) "1 entrega em risco" else "$risks entregas em risco")
        if (extra.isEmpty()) return base
        val prefix = base.trim().removeSuffix(".")
        val parts = mutableListOf<String>()
        if (prefix.isNotEmpty()) {
            val lastJoin = prefix.lastIndexOf(" e ")
            parts.add(if (lastJoin >= 0) prefix.substring(0, lastJoin) + ", " + prefix.substring(lastJoin + 3) else prefix)
        }
        parts.addAll(extra)
        return if (parts.size == 1) "${parts[0]}." else "${parts.dropLast(1).joinToString(", ")} e ${parts.last()}."
    }
}
