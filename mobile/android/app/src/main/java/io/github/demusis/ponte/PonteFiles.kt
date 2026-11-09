package io.github.demusis.ponte

import android.content.ActivityNotFoundException
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Intent
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import android.widget.Toast
import java.io.File
import java.io.FileNotFoundException

/**
 * Minimal file provider (no AndroidX). Serves only files directly inside cacheDir/compartilhar
 * and cacheDir/fotos, addressed by name: content://AUTHORITY/<dir>/<name>.
 */
class PonteFiles : ContentProvider() {

    override fun onCreate() = true

    private fun resolve(uri: Uri): File? {
        val ctx = context ?: return null
        val seg = uri.pathSegments
        if (seg.size != 2) return null
        val dir = seg[0]
        val name = seg[1]
        if (dir != DIR_SHARED && dir != DIR_PHOTOS) return null
        if (name.isEmpty() || name.contains('/') || name.contains('\\') ||
            name.contains("..") || name.contains('\u0000')
        ) return null
        return File(File(ctx.cacheDir, dir), name)
    }

    override fun getType(uri: Uri): String? {
        val f = resolve(uri) ?: return null
        val ext = f.extension.lowercase()
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
    }

    override fun query(
        uri: Uri, projection: Array<out String>?, selection: String?,
        selectionArgs: Array<out String>?, sortOrder: String?
    ): Cursor? {
        val f = resolve(uri)
        if (f == null || !f.isFile) return null
        val cols: Array<String> =
            projection?.map { it }?.toTypedArray() ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val out = MatrixCursor(cols)
        val row = out.newRow()
        for (c in cols) {
            when (c) {
                OpenableColumns.DISPLAY_NAME -> row.add(f.name)
                OpenableColumns.SIZE -> row.add(f.length())
                else -> row.add(null)
            }
        }
        return out
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor? {
        val f = resolve(uri) ?: throw FileNotFoundException("Arquivo não permitido.")
        val pfdMode = ParcelFileDescriptor.parseMode(mode)
        if (mode.contains('w') && uri.pathSegments.firstOrNull() != DIR_PHOTOS) {
            throw FileNotFoundException("Somente leitura.")
        }
        if (mode.contains('w')) f.parentFile?.mkdirs() else if (!f.isFile) throw FileNotFoundException(f.name)
        return ParcelFileDescriptor.open(f, pfdMode)
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? =
        throw UnsupportedOperationException()

    override fun update(
        uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?
    ): Int = throw UnsupportedOperationException()

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int =
        throw UnsupportedOperationException()

    companion object {
        const val AUTHORITY = "io.github.demusis.ponte.files"
        const val DIR_SHARED = "compartilhar"
        const val DIR_PHOTOS = "fotos"

        fun uriFor(dir: String, name: String): Uri =
            Uri.Builder().scheme("content").authority(AUTHORITY).appendPath(dir).appendPath(name).build()
    }
}

/** Exports a PDF/attachment into cacheDir/compartilhar and opens it with another app. */
internal fun openExported(a: MainActivity, kind: String, id: Long, captureId: Long) {
    val dir = File(a.cacheDir, PonteFiles.DIR_SHARED)
    Toast.makeText(a, "Preparando o arquivo...", Toast.LENGTH_SHORT).show()
    SyncRunner.background({
        dir.mkdirs()
        val old = System.currentTimeMillis() - 5L * 60 * 1000
        dir.listFiles()?.forEach { if (it.isFile && it.lastModified() < old) it.delete() }
        val r = Api.exportFile(kind, id, captureId, dir.absolutePath)
        val src = File(r.s("path"))
        val safe = src.name.replace("..", "_").replace('/', '_').replace('\\', '_').ifEmpty { "arquivo" }
        val dest = File(dir, safe)
        if (src.canonicalPath != dest.canonicalPath) src.copyTo(dest, overwrite = true)
        var mime = r.s("mime", "application/octet-stream").ifEmpty { "application/octet-stream" }
        if (mime == "application/octet-stream") {
            MimeTypeMap.getSingleton().getMimeTypeFromExtension(dest.extension.lowercase())?.let { mime = it }
        }
        Pair(safe, mime)
    }, { res ->
        val act = MainActivity.live ?: a
        try {
            val i = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(PonteFiles.uriFor(PonteFiles.DIR_SHARED, res.first), res.second)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            act.startActivity(i)
        } catch (_: ActivityNotFoundException) {
            val ext = res.first.substringAfterLast('.', "")
            Toast.makeText(
                act, "Nenhum aplicativo neste aparelho abre arquivos" + (if (ext.isEmpty()) "." else " .$ext."),
                Toast.LENGTH_LONG
            ).show()
        } catch (e: Throwable) {
            Toast.makeText(act, e.message ?: e.toString(), Toast.LENGTH_LONG).show()
        }
    }, { Toast.makeText(MainActivity.live ?: a, it, Toast.LENGTH_LONG).show() })
}
