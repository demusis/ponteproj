package io.github.demusis.ponte

import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

internal data class Version(val major: Long, val minor: Long, val patch: Long, val pre: List<String>) : Comparable<Version> {
    override fun compareTo(other: Version): Int {
        compareValuesBy(this, other, Version::major, Version::minor, Version::patch).let { if (it != 0) return it }
        if (pre.isEmpty() || other.pre.isEmpty()) return when {
            pre.isEmpty() && other.pre.isEmpty() -> 0
            pre.isEmpty() -> 1
            else -> -1
        }
        for (i in 0 until minOf(pre.size, other.pre.size)) {
            val a = pre[i]; val b = other.pre[i]
            val an = a.all { it in '0'..'9' }; val bn = b.all { it in '0'..'9' }
            val c = when {
                an && bn -> compareValues(a.length, b.length).takeIf { it != 0 } ?: a.compareTo(b)
                an -> -1
                bn -> 1
                else -> a.compareTo(b)
            }
            if (c != 0) return c
        }
        return pre.size.compareTo(other.pre.size)
    }
}

internal fun parseVersion(text: String): Version? {
    val match = Regex("""^v?(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$""").matchEntire(text) ?: return null
    val pre = match.groupValues[4].let { if (it.isEmpty()) emptyList() else it.split('.') }
    if (pre.any { it.length > 1 && it[0] == '0' && it.all { c -> c in '0'..'9' } }) return null
    return Version(match.groupValues[1].toLongOrNull() ?: return null, match.groupValues[2].toLongOrNull() ?: return null, match.groupValues[3].toLongOrNull() ?: return null, pre)
}

internal fun selectApkVersion(names: List<String>): String? = names.mapNotNull { name ->
    if (!name.startsWith("PonteDeBolso-") || !name.endsWith(".apk")) null
    else name.removePrefix("PonteDeBolso-").removeSuffix(".apk").let { v -> parseVersion(v)?.let { v to it } }
}.maxByOrNull { it.second }?.first

internal sealed interface UpdateResult {
    data class Current(val version: String) : UpdateResult
    data class Newer(val version: String, val url: String) : UpdateResult
    data object Error : UpdateResult
}

internal object UpdateCheck {
    private const val API = "https://api.github.com/repos/demusis/ponteproj/releases/latest"
    private const val FALLBACK = "https://github.com/demusis/ponteproj/releases/latest"
    internal fun safeUrl(url: String): String {
        val prefix = "https://github.com/demusis/ponteproj/releases/"
        val tail = url.removePrefix(prefix)
        return if (url.startsWith(prefix) && tail.isNotEmpty() && !tail.contains("..") && tail.all { it.isLetterOrDigit() && it.code < 128 || it in "._/-" }) url else FALLBACK
    }
    fun check(installed: String): UpdateResult = try {
        val connection = URL(API).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = "GET"
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 10_000
            connection.readTimeout = 10_000
            connection.setRequestProperty("User-Agent", "PonteDeBolso")
            connection.setRequestProperty("Accept", "application/vnd.github+json")
            if (connection.responseCode !in 200..299) UpdateResult.Error else {
                val bytes = connection.inputStream.use { input ->
                    val output = java.io.ByteArrayOutputStream()
                    val buffer = ByteArray(4096)
                    while (output.size() <= 256 * 1024) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                    }
                    output.toByteArray()
                }
                if (bytes.size > 256 * 1024) UpdateResult.Error else {
                    val json = JSONObject(String(bytes, Charsets.UTF_8))
                    val assets = json.getJSONArray("assets")
                    val names = (0 until assets.length()).map { assets.getJSONObject(it).getString("name") }
                    val latest = selectApkVersion(names)
                    val currentVersion = parseVersion(installed)
                    val latestVersion = latest?.let(::parseVersion)
                    if (latest == null || latestVersion == null || currentVersion == null) UpdateResult.Error
                    else if (latestVersion > currentVersion) UpdateResult.Newer(latest, safeUrl(json.getString("html_url")))
                    else UpdateResult.Current(installed)
                }
            }
        } finally { connection.disconnect() }
    } catch (_: Exception) { UpdateResult.Error }
}
