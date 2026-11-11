package io.github.demusis.ponte

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream

/** The rules for what comes from other apps (ShareRules.kt): plain values in, a decision out. */
class ShareRulesTest {
    private val own = "io.github.demusis.ponte.files"

    private fun check(
        action: String? = ShareRules.ACTION_SEND, mime: String? = "application/pdf",
        scheme: String? = "content", authority: String? = "com.android.providers.media.documents",
        count: Int = 1, text: Boolean = false
    ) = ShareRules.check(action, mime, scheme, authority, count, text, own)

    @Test fun oneContentFileIsAccepted() {
        assertTrue(check().ok)
        assertTrue(check(scheme = "CONTENT", mime = null).ok)
        // The type is only a hint: any plausible one is accepted, the core judges the content.
        assertTrue(check(mime = "application/x-whatever").ok)
        assertTrue(check(mime = "*/*").ok)
    }

    @Test fun textAloneIsAccepted() {
        assertTrue(check(mime = "text/plain", scheme = null, authority = null, count = 0, text = true).ok)
    }

    @Test fun nothingToKeepIsRefused() {
        val v = check(mime = "text/plain", scheme = null, authority = null, count = 0, text = false)
        assertEquals(ShareRules.Reason.EMPTY, v.reason)
        assertEquals(ShareRules.MSG_EMPTY, v.message)
    }

    @Test fun severalFilesAreRefusedPolitely() {
        val multiple = check(action = ShareRules.ACTION_SEND_MULTIPLE, count = 3)
        assertEquals(ShareRules.Reason.MULTIPLE, multiple.reason)
        assertEquals("Compartilhe um arquivo de cada vez.", multiple.message)
        assertEquals(ShareRules.Reason.MULTIPLE, check(count = 2).reason)
        // Even a SEND_MULTIPLE with one item is refused: one at a time, by its action.
        assertEquals(ShareRules.Reason.MULTIPLE, check(action = ShareRules.ACTION_SEND_MULTIPLE, count = 1).reason)
    }

    @Test fun otherActionsAreRefused() {
        for (a in listOf(null, "", "android.intent.action.VIEW", "android.intent.action.MAIN", "send")) {
            assertEquals(a.toString(), ShareRules.Reason.NOT_SENT, check(action = a).reason)
        }
    }

    @Test fun onlyContentUrisAreAccepted() {
        for (s in listOf("file", "http", "https", "ftp", "android.resource", "data", "intent", "javascript", "", null)) {
            val v = check(scheme = s, authority = "x")
            assertEquals(s.toString(), ShareRules.Reason.NOT_CONTENT, v.reason)
        }
        // A file:// Uri has no authority and points into the phone's own folders: refused as not content.
        assertEquals(ShareRules.Reason.NOT_CONTENT, check(scheme = "file", authority = null).reason)
        assertEquals(ShareRules.Reason.NOT_CONTENT, check(authority = null).reason)
        assertEquals(ShareRules.Reason.NOT_CONTENT, check(authority = "  ").reason)
        assertEquals(ShareRules.MSG_NOT_CONTENT, check(scheme = "file").message)
    }

    @Test fun theAppsOwnProviderIsNeverFollowed() {
        for (a in listOf(own, own.uppercase(), "10@$own", "10@$own:80", " $own ", "$own.evil", "evil.$own")) {
            val v = check(authority = a)
            assertEquals(a, ShareRules.Reason.OWN_FILE, v.reason)
            assertEquals(ShareRules.MSG_OWN_FILE, v.message)
        }
        assertTrue(check(authority = "com.google.android.apps.docs.storage").ok)
        assertTrue(check(authority = "media").ok)
    }

    @Test fun malformedTypesAreRefused() {
        for (m in listOf("", "text plain", "a/b/c", "x".repeat(300), "text/plain\n")) {
            assertEquals(m, ShareRules.Reason.BAD_TYPE, check(mime = m).reason)
        }
        assertTrue(ShareRules.plausibleMime("image/jpeg"))
        assertTrue(ShareRules.plausibleMime("image/*"))
    }

    @Test fun ownDirectoriesAreRecognisedByRealPath() {
        val dirs = listOf("/data/user/0/io.github.demusis.ponte", "/data/data/io.github.demusis.ponte/")
        assertTrue(ShareRules.insideDirs("/data/user/0/io.github.demusis.ponte/files/acervo.sqlite3", dirs))
        assertTrue(ShareRules.insideDirs("/data/data/io.github.demusis.ponte/cache/x", dirs))
        assertTrue(ShareRules.insideDirs("/data/user/0/io.github.demusis.ponte", dirs))
        assertFalse(ShareRules.insideDirs("/data/user/0/io.github.demusis.ponte2/files/x", dirs))
        assertFalse(ShareRules.insideDirs("/storage/emulated/0/Download/x.pdf", dirs))
        assertFalse(ShareRules.insideDirs("pipe:[1234]", dirs))
        assertFalse(ShareRules.insideDirs(null, dirs))
        assertFalse(ShareRules.insideDirs("/data/x", listOf("")))
    }

    @Test fun anImageHintAllowsShrinking() {
        assertTrue(ShareRules.looksLikeImage("image/jpeg"))
        assertTrue(ShareRules.looksLikeImage("IMAGE/PNG"))
        assertFalse(ShareRules.looksLikeImage("application/pdf"))
        assertFalse(ShareRules.looksLikeImage(null))
    }

    // ------------------------------------------------------------------ names

    @Test fun namesLoseFoldersAndInvisibleCharacters() {
        assertEquals("oficio.pdf", ShareRules.cleanName("oficio.pdf", 180))
        assertEquals("oficio.pdf", ShareRules.cleanName("../../etc/oficio.pdf", 180))
        assertEquals("oficio.pdf", ShareRules.cleanName("C:\\Users\\x\\oficio.pdf", 180))
        assertEquals("oficio.pdf", ShareRules.cleanName("  oficio.pdf. ", 180))
        assertEquals("oficio.pdf", ShareRules.cleanName("ofi\u0000cio\n.pdf", 180))
        assertEquals("", ShareRules.cleanName(null, 180))
        assertEquals("", ShareRules.cleanName("   ", 180))
        assertEquals("", ShareRules.cleanName("/", 180))
        // A direction override (the "exe.fdp" trick) is dropped, so the name shows what it is.
        assertEquals("reciboexe.pdf", ShareRules.cleanName("recibo\u202Eexe.pdf", 180))
        // Names that start with dots do not become hidden files.
        assertEquals("segredo.txt", ShareRules.cleanName("..segredo.txt", 180))
        assertEquals("Relatório de gestão.docx", ShareRules.cleanName("Relatório de gestão.docx", 180))
    }

    @Test fun longNamesAreCutKeepingTheExtension() {
        val long = "a".repeat(300) + ".pdf"
        val cut = ShareRules.cleanName(long, 180)
        assertEquals(180, cut.length)
        assertTrue(cut.endsWith(".pdf"))
        assertEquals(180, ShareRules.cleanName("b".repeat(200), 180).length)
        assertEquals("a.pdf", ShareRules.cleanName("abc.pdf", 5))
        // Exactly the limit is kept whole.
        assertEquals("a".repeat(176) + ".pdf", ShareRules.cleanName("a".repeat(176) + ".pdf", 180))
        // Characters outside the basic plane count once, and are not cut in half.
        val emoji = "\uD83D\uDE00".repeat(200) + ".txt"
        val cutEmoji = ShareRules.cleanName(emoji, 180)
        assertEquals(180, cutEmoji.codePointCount(0, cutEmoji.length))
        assertTrue(cutEmoji.endsWith(".txt"))
    }

    @Test fun aNameWithoutExtensionBorrowsOneFromTheType() {
        val ext = { m: String -> mapOf("image/jpeg" to "jpg", "application/pdf" to "pdf", "text/plain" to "txt")[m] }
        assertEquals("IMG_2031.jpg", ShareRules.withExtension("IMG_2031", "image/jpeg", ext))
        assertEquals("compartilhado.pdf", ShareRules.withExtension("", "application/pdf", ext))
        assertEquals("nota.txt", ShareRules.withExtension("nota", "TEXT/PLAIN; charset=utf-8", ext))
        // A name that has an extension is never changed, whatever the hint says.
        assertEquals("foto.png", ShareRules.withExtension("foto.png", "image/jpeg", ext))
        // Nothing known: left as it is (the core will say the type cannot be checked).
        assertEquals("dados", ShareRules.withExtension("dados", "application/x-unknown", ext))
        assertEquals("", ShareRules.withExtension("", null, ext))
        assertEquals("a.jpg", ShareRules.withExtension("a", "image/jpeg") { "..jpg" })
        assertEquals("a", ShareRules.withExtension("a", "image/jpeg") { "j p/g" })
    }

    @Test fun extensionsAreReplacedAndTitlesComeFromNames() {
        assertEquals("foto.jpg", ShareRules.replaceExtension("foto.png", "jpg"))
        assertEquals("foto.jpg", ShareRules.replaceExtension("foto", ".jpg"))
        assertEquals("a.b.jpg", ShareRules.replaceExtension("a.b.heic", "jpg"))
        assertEquals("Recibo de entrega", ShareRules.titleFromName("Recibo de entrega.pdf"))
        assertEquals("a.b", ShareRules.titleFromName("a.b.pdf"))
        assertEquals("semextensao", ShareRules.titleFromName("semextensao"))
    }

    // ------------------------------------------------------------------ texts

    @Test fun textIsPlainAndCut() {
        assertEquals("a\nb", ShareRules.plainText("  a\r\nb\r  ", 1000))
        assertEquals("a\tb", ShareRules.plainText("a\u0000\tb\u0007", 1000))
        assertEquals("", ShareRules.plainText(null, 10))
        assertEquals("", ShareRules.plainText("   ", 10))
        // Markup and addresses stay exactly what they are: text.
        assertEquals("<b>negrito</b> https://exemplo.org/x?a=1", ShareRules.plainText("<b>negrito</b> https://exemplo.org/x?a=1", 1000))
        // Cut by bytes of UTF-8, never inside a character: "é" is two bytes.
        assertEquals("abc", ShareRules.plainText("abcdef", 3))
        assertEquals("aé", ShareRules.plainText("aéé", 4))
        assertEquals("a", ShareRules.plainText("aéé", 2))
        assertEquals("", ShareRules.plainText("é", 1))
        assertEquals("\uD83D\uDE00", ShareRules.plainText("\uD83D\uDE00\uD83D\uDE00", 4))
        assertEquals("x".repeat(100), ShareRules.plainText("x".repeat(500), 100))
    }

    @Test fun cuttingAtTheCoreLimitIsExact() {
        val limit = 500 * 1024
        val text = "ab".repeat(limit)
        val cut = ShareRules.plainText(text, limit)
        assertEquals(limit, cut.toByteArray(Charsets.UTF_8).size)
        assertEquals(limit, ShareRules.plainText("x".repeat(limit), limit).length)
        assertEquals(limit, ShareRules.plainText("x".repeat(limit + 1), limit).length)
    }

    @Test fun aSubjectIsOneLine() {
        assertEquals("Assunto do e-mail com quebra", ShareRules.oneLine("Assunto do e-mail\ncom\r\n quebra ", 1000))
        assertEquals("", ShareRules.oneLine(null, 1000))
        assertEquals(1000, ShareRules.oneLine("x".repeat(5000), 1000).length)
        assertEquals("aé", ShareRules.oneLine("aéé", 4))
        assertEquals("ab", ShareRules.oneLine("a\u202Eb", 100))
    }

    @Test fun descriptionsAreCutByCharacters() {
        assertEquals("abc", ShareRules.capChars("abc", 500))
        assertEquals(500, ShareRules.capChars("a".repeat(900), 500).length)
        val e = ShareRules.capChars("\uD83D\uDE00".repeat(10), 3)
        assertEquals(3, e.codePointCount(0, e.length))
    }

    @Test fun thePreviewShowsTheFirstLines() {
        assertEquals("um\ndois", ShareRules.preview("um\n\n  dois  \n", 6))
        assertEquals("um\ndois…", ShareRules.preview("um\ndois\ntres", 2))
        assertTrue(ShareRules.preview("x".repeat(1000), 6, 400).endsWith("…"))
        assertEquals(401, ShareRules.preview("x".repeat(1000), 6, 400).length)
        assertEquals("", ShareRules.preview("", 6))
    }

    @Test fun theTooBigMessageSaysTheLimit() {
        val m = ShareRules.tooBigMessage("grande.pdf", 8L * 1024 * 1024)
        assertTrue(m, m.startsWith("O arquivo “grande.pdf” passa do limite de 8 MB."))
        assertTrue(ShareRules.tooBigMessage("", 8L * 1024 * 1024).startsWith("O arquivo passa do limite de 8 MB."))
    }

    // ------------------------------------------------------------------ the streaming cap

    /** An endless stream of zeros that counts what was read from it. */
    private class Endless : InputStream() {
        var read = 0L
        override fun read(): Int { read++; return 0 }
        override fun read(b: ByteArray, off: Int, len: Int): Int { read += len; return len }
    }

    @Test fun theCapAcceptsExactlyTheLimit() {
        val out = ByteArrayOutputStream()
        assertEquals(100L, BoundedCopy.copy(ByteArrayInputStream(ByteArray(100)), out, 100))
        assertEquals(100, out.size())
        val empty = ByteArrayOutputStream()
        assertEquals(0L, BoundedCopy.copy(ByteArrayInputStream(ByteArray(0)), empty, 100))
        // Bigger than the copy buffer, still exact.
        val big = ByteArrayOutputStream()
        val size = 200_000
        assertEquals(size.toLong(), BoundedCopy.copy(ByteArrayInputStream(ByteArray(size)), big, size.toLong()))
        assertEquals(size, big.size())
    }

    @Test fun oneByteOverStopsTheCopyWithoutWritingIt() {
        for (extra in listOf(1, 2, 70_000)) {
            val out = ByteArrayOutputStream()
            try {
                BoundedCopy.copy(ByteArrayInputStream(ByteArray(100 + extra)), out, 100)
                fail("should stop")
            } catch (e: TooBigException) {
                assertEquals(100L, e.limit)
            }
            assertTrue("never more than the limit is written", out.size() <= 100)
        }
        val big = ByteArrayOutputStream()
        val size = 200_000
        try {
            BoundedCopy.copy(ByteArrayInputStream(ByteArray(size + 1)), big, size.toLong())
            fail("should stop")
        } catch (_: TooBigException) {
        }
        assertEquals(size, big.size())
    }

    @Test fun anEndlessStreamIsNeverReadToItsEnd() {
        val src = Endless()
        val out = ByteArrayOutputStream()
        try {
            BoundedCopy.copy(src, out, 1000)
            fail("should stop")
        } catch (_: TooBigException) {
        }
        assertTrue("stopped after one buffer: ${src.read}", src.read <= 64 * 1024)
        assertEquals(1000, out.size())
    }
}
