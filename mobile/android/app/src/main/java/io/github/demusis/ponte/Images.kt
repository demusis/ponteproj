package io.github.demusis.ponte

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.media.ExifInterface
import java.io.ByteArrayOutputStream
import java.io.File
import kotlin.math.max
import kotlin.math.roundToInt

internal object Images {
    private const val LIMIT = 7 * 1024 * 1024

    /** Decodes with inSampleSize, scales so the long side is at most maxSide, applies EXIF rotation. */
    fun decode(f: File, maxSide: Int): Bitmap {
        val path = f.absolutePath
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) throw PonteException("A foto não pôde ser lida. Tire outra foto.")
        val longSide = max(bounds.outWidth, bounds.outHeight)
        var sample = 1
        while (longSide / (sample * 2) >= maxSide) sample *= 2
        var bmp = BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inSampleSize = sample })
            ?: throw PonteException("A foto não pôde ser lida. Tire outra foto.")
        val ls = max(bmp.width, bmp.height)
        if (ls > maxSide) {
            val k = maxSide.toFloat() / ls
            bmp = Bitmap.createScaledBitmap(
                bmp, (bmp.width * k).roundToInt().coerceAtLeast(1),
                (bmp.height * k).roundToInt().coerceAtLeast(1), true
            )
        }
        val exif = try {
            ExifInterface(path).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL)
        } catch (_: Throwable) {
            ExifInterface.ORIENTATION_NORMAL
        }
        val m = Matrix()
        when (exif) {
            2 -> m.postScale(-1f, 1f)
            3 -> m.postRotate(180f)
            4 -> m.postScale(1f, -1f)
            5 -> { m.postRotate(90f); m.postScale(-1f, 1f) }
            6 -> m.postRotate(90f)
            7 -> { m.postRotate(-90f); m.postScale(-1f, 1f) }
            8 -> m.postRotate(-90f)
        }
        if (!m.isIdentity) {
            val r = Bitmap.createBitmap(bmp, 0, 0, bmp.width, bmp.height, m, true)
            if (r !== bmp) bmp.recycle()
            bmp = r
        }
        return bmp
    }

    /** Writes a JPEG (<= 2000 px long side, quality 85, under 7 MB) to dst. */
    fun resizeForUpload(src: File, dst: File) {
        var side = 2000
        var quality = 85
        while (side >= 400) {
            val bmp = decode(src, side)
            val bos = ByteArrayOutputStream()
            bmp.compress(Bitmap.CompressFormat.JPEG, quality, bos)
            bmp.recycle()
            if (bos.size() < LIMIT) {
                dst.writeBytes(bos.toByteArray())
                return
            }
            if (quality > 50) quality -= 15 else side = (side * 0.8).toInt()
        }
        throw PonteException("A foto é grande demais para ser guardada. Tire outra foto.")
    }
}
