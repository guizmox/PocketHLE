package com.pockethle.app

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import org.json.JSONObject
import java.io.File

/** ZIP bytes and restore semantics are shared with desktop through Rust. */
object LibraryBackup {
    private fun native(root: String, file: File, mode: Int) {
        val response = JSONObject(NativeBridge.libraryBackup(root, file.absolutePath, mode))
        check(response.optBoolean("ok", false)) { response.optString("error", "Backup failed") }
    }

    fun save(context: Context, root: String): String {
        val zip = File.createTempFile("pockethle-backup-", ".zip", context.cacheDir)
        val name = "PocketHLE-backup-${System.currentTimeMillis()}.zip"
        try {
            native(root, zip, -1)
            if (Build.VERSION.SDK_INT >= 29) {
                val resolver = context.contentResolver
                val values = ContentValues().apply {
                    put(MediaStore.Downloads.DISPLAY_NAME, name)
                    put(MediaStore.Downloads.MIME_TYPE, "application/zip")
                    put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
                    put(MediaStore.Downloads.IS_PENDING, 1)
                }
                val uri = checkNotNull(resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)) { "Could not create Downloads backup" }
                try {
                    checkNotNull(resolver.openOutputStream(uri, "w")) { "Could not open Downloads backup" }.use { output ->
                        zip.inputStream().use { it.copyTo(output) }
                    }
                    check(resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null) == 1) {
                        "Could not publish Downloads backup"
                    }
                } catch (error: Exception) {
                    resolver.delete(uri, null, null)
                    throw error
                }
            } else {
                @Suppress("DEPRECATION")
                val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                check(downloads.isDirectory || downloads.mkdirs()) { "Downloads is unavailable" }
                val destination = File(downloads, name)
                check(destination.createNewFile()) { "Backup name already exists" }
                try {
                    zip.inputStream().use { input -> destination.outputStream().use { input.copyTo(it) } }
                    android.media.MediaScannerConnection.scanFile(context, arrayOf(destination.absolutePath), arrayOf("application/zip"), null)
                } catch (error: Exception) { destination.delete(); throw error }
            }
            return name
        } finally { zip.delete() }
    }

    fun restore(context: Context, root: String, uri: Uri, mode: Int) {
        require(mode in 0..2)
        val zip = File.createTempFile("pockethle-restore-", ".zip", context.cacheDir)
        try {
            checkNotNull(context.contentResolver.openInputStream(uri)) { "Could not open backup" }.use { input ->
                zip.outputStream().use { input.copyTo(it) }
            }
            native(root, zip, mode)
        } finally { zip.delete() }
    }
}
