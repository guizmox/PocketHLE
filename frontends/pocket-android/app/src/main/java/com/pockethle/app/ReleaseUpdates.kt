package com.pockethle.app

import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URI

/** One bounded, background update check per process; never retains an Activity. */
object ReleaseUpdates {
    private const val RELEASES = "https://github.com/guizmox/PocketHLE/releases/"
    private const val REBORN_RELEASES = "https://github.com/guizmox/PocketHLE-Reborn/releases/"
    private val endpoints = listOf(
        "https://api.github.com/repos/guizmox/PocketHLE-Reborn/releases/latest",
        "https://api.github.com/repos/guizmox/PocketHLE/releases/latest",
    )
    private var checked = false
    private var cached: Update? = null
    @Volatile var announced = false
    data class Update(val version: String, val url: String)

    @Synchronized fun check(installed: String): Update? {
        if (checked) return cached
        checked = true
        val deadline = System.nanoTime() + 8_000_000_000L
        for (endpoint in endpoints) {
            val release = fetch(endpoint, installed, deadline) ?: continue
            cached = parse(release, installed)
            break
        }
        return cached
    }

    private fun fetch(endpoint: String, installed: String, deadline: Long): String? =
        runCatching {
            val connection = URI(endpoint).toURL().openConnection() as HttpURLConnection

            try {
                val remainingMs = (deadline - System.nanoTime()) / 1_000_000L
                if (remainingMs <= 0) return@runCatching null
                connection.connectTimeout = remainingMs.coerceAtMost(4000).toInt()
                connection.readTimeout = connection.connectTimeout
                connection.instanceFollowRedirects = false
                connection.setRequestProperty("User-Agent", "PocketHLE-Reborn/$installed")
                connection.setRequestProperty("Accept", "application/vnd.github+json")
                if (connection.responseCode != 200) return@runCatching null
                val output = ByteArrayOutputStream()
                connection.inputStream.use { input ->
                    val buffer = ByteArray(8192)
                    while (true) {
                        val remainingMs = (deadline - System.nanoTime()) / 1_000_000L
                        if (remainingMs <= 0) return@runCatching null
                        connection.readTimeout = remainingMs.coerceAtMost(4000).toInt()
                        val count = input.read(buffer)
                        if (count < 0) break
                        if (output.size() + count > 1_048_576) return@runCatching null
                        output.write(buffer, 0, count)
                    }
                }
                output.toString("UTF-8").also { JSONObject(it).getString("tag_name") }
            } finally { connection.disconnect() }
        }.getOrNull()

    internal fun parse(json: String, installed: String): Update? = runCatching {
        val release = JSONObject(json)
        if (release.getBoolean("draft") || release.getBoolean("prerelease")) return@runCatching null
        val tag = release.getString("tag_name")
        val remote = version(tag.removePrefix("v")) ?: return@runCatching null
        val current = version(installed) ?: return@runCatching null
        if (remote.second.isNotEmpty()) return@runCatching null
        val difference = remote.first.zip(current.first).firstOrNull { it.first != it.second }
        if (difference != null && difference.first < difference.second) return@runCatching null
        if (difference == null && current.second.isEmpty()) return@runCatching null
        val assets = release.optJSONArray("assets")
        val downloads = (0 until (assets?.length() ?: 0)).mapNotNull { index ->
            val asset = assets!!.getJSONObject(index)
            val url = asset.getString("browser_download_url")
            url.takeIf { asset.getString("name").endsWith(".apk", ignoreCase = true) &&
                listOf(REBORN_RELEASES, RELEASES).any { base -> it.startsWith("${base}download/") } }
        }
        val base = if (release.optString("html_url").startsWith(REBORN_RELEASES)) REBORN_RELEASES else RELEASES
        Update(tag.removePrefix("v"), downloads.singleOrNull() ?: "${base}tag/$tag")
    }.getOrNull()

    private fun version(text: String): Pair<List<Long>, String>? {
        val number = "(0|[1-9][0-9]*)"
        val match = Regex("^$number\\.$number\\.$number(?:-([0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*))?(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$")
            .matchEntire(text) ?: return null
        val core = (1..3).map { match.groupValues[it].toLongOrNull() ?: return null }
        val pre = match.groupValues[4]
        if (pre.split('.').any { it.length > 1 && it.all(Char::isDigit) && it.startsWith('0') }) return null
        return core to pre
    }
}
