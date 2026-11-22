package io.github.demusis.ponte

import org.json.JSONObject

/** A record of this phone that the library numbered differently on a sync, still to be read (see PonteCore.renumberNotices). */
internal data class RenumberNotice(
    val id: Long,
    val title: String,
    val from: String,
    val to: String,
    /** The sentence the core wrote, ready to show. */
    val sentence: String,
    /** The record to open; 0 when it cannot be opened. */
    val documentId: Long,
    val at: String = ""
)

internal fun JSONObject.toRenumberNotice() = RenumberNotice(
    id = optLong("id"), title = s("title"), from = s("from"), to = s("to"), sentence = s("sentence"),
    documentId = optLong("document_id"), at = s("at")
)

/** Pure rules of the card of Hoje about renumbered records, and of the "Número provisório" marker. */
internal object RenumberRules {
    const val TITLE = "Registro com número alterado"
    const val OPEN = "Abrir o registro"
    const val DISMISS = "Entendi"

    /** The most notices the card spells out (the newest ones); the others are counted. */
    const val MAX_SHOWN = 3

    const val PROVISIONAL = "Número provisório"
    const val PROVISIONAL_NOTE = "Número provisório: pode mudar ao sincronizar."

    /** The notices the card shows: the newest [MAX_SHOWN] (the core lists the newest first). */
    fun shown(notices: List<RenumberNotice>): List<RenumberNotice> = notices.take(MAX_SHOWN)

    /** "e mais 2" for the notices left out; empty when all are shown. */
    fun moreText(notices: List<RenumberNotice>): String {
        val more = notices.size - MAX_SHOWN
        return if (more > 0) "e mais $more" else ""
    }

    /** "Abrir o registro" only for a notice whose record can be opened. */
    fun canOpen(notice: RenumberNotice) = notice.documentId > 0

    /** What the screen reader says for the buttons of one notice (several look alike on the card). */
    fun openDescription(notice: RenumberNotice) = "$OPEN ${notice.title.ifBlank { notice.to }}".trim()
    fun dismissDescription(notice: RenumberNotice) = "$DISMISS: ${notice.sentence}".trim()

    /** The muted line of a row of the documents list: the number (or origin), then the marker when the number may still change. */
    fun rowSub(sub: String, provisional: Boolean): String =
        if (!provisional) sub else if (sub.isEmpty()) PROVISIONAL else "$sub · $PROVISIONAL"

    /** The line over the document: the number with the marker. */
    fun headerLine(number: String, provisional: Boolean): String = rowSub(number, provisional)
}
