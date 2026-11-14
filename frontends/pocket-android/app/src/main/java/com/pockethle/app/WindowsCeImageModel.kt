package com.pockethle.app

import android.app.Application
import android.net.Uri
import android.provider.DocumentsContract
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.MutableLiveData
import androidx.lifecycle.viewModelScope
import java.io.File
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Copy a selected CE filesystem using read-only SAF access, off the UI thread. */
class WindowsCeImageModel(application: Application) : AndroidViewModel(application) {
    sealed class Step {
        data object Idle : Step()
        data object Copying : Step()
        data class Imported(val imageRoot: String, val dataDir: String) : Step()
        data class Failed(val message: String) : Step()
    }
    val step = MutableLiveData<Step>(Step.Idle)

    fun importImage(tree: Uri) {
        if (step.value == Step.Copying) return
        step.value = Step.Copying
        viewModelScope.launch {
            try {
                val result = withContext(Dispatchers.IO) {
                    val ctx = getApplication<Application>()
                    val images = File(LibraryPaths.root(ctx), "ce-images").canonicalFile
                    check(images.mkdirs() || images.isDirectory) { "Could not create the image folder" }
                    val id = UUID.randomUUID().toString()
                    val staging = File(images, ".import-$id")
                    val destination = File(images, id)
                    check(staging.mkdir()) { "Could not create the temporary image folder" }
                    try {
                        val visited = HashSet<String>()
                        suspend fun copyFolder(document: String, target: File, depth: Int) {
                            ensureActive()
                            check(depth < 64 && visited.add(document)) { "Invalid directory tree" }
                            val names = HashSet<String>()
                            val children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, document)
                            ctx.contentResolver.query(children, arrayOf(
                                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                                DocumentsContract.Document.COLUMN_MIME_TYPE,
                            ), null, null, null)?.use { cursor ->
                                while (cursor.moveToNext()) {
                                    ensureActive()
                                    val childId = cursor.getString(0)
                                    val name = cursor.getString(1)
                                    check(name.isNotBlank() && name != "." && name != ".."
                                        && !name.contains('/') && !name.contains('\\') && !name.contains('\u0000')
                                        && names.add(name.lowercase())) { "Invalid or duplicate filename in the image" }
                                    // Normalize the filesystem's Windows folder on case-sensitive hosts.
                                    val file = File(target, if (name.equals("Windows", true)) "Windows" else name)
                                    check(file.canonicalFile.parentFile == target.canonicalFile) { "Invalid image path" }
                                    if (cursor.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                                        check(file.mkdir()) { "Could not create $name" }
                                        copyFolder(childId, file, depth + 1)
                                    } else {
                                        val uri = DocumentsContract.buildDocumentUriUsingTree(tree, childId)
                                        ctx.contentResolver.openInputStream(uri)?.use { input ->
                                            file.outputStream().use { input.copyTo(it) }
                                        } ?: error("Could not read $name")
                                    }
                                }
                            } ?: error("Could not read the selected folder")
                        }
                        copyFolder(DocumentsContract.getTreeDocumentId(tree), staging, 0)
                        val filesystem = when {
                            File(staging, "Windows").isDirectory -> staging
                            File(staging, "fs/Windows").isDirectory -> File(staging, "fs")
                            else -> error("Select the extracted filesystem containing Windows/coredll.dll, or its parent containing fs/Windows.")
                        }
                        val coredll = File(filesystem, "Windows").listFiles()?.firstOrNull { it.name.equals("coredll.dll", true) && it.isFile }
                            ?: error("The image has no Windows/coredll.dll")
                        if (coredll.name != "coredll.dll") check(coredll.renameTo(File(coredll.parentFile, "coredll.dll")))
                        ensureActive()
                        check(staging.renameTo(destination)) { "Could not finish importing the image" }
                        val suffix = if (filesystem == staging) "" else "/fs"
                        Step.Imported("ce-images/$id$suffix", "ce-data/$id")
                    } finally {
                        // Only this transaction's temporary copy can be removed.
                        if (staging.canonicalFile.parentFile == images && staging.name == ".import-$id") staging.deleteRecursively()
                    }
                }
                step.value = result
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                step.value = Step.Failed(error.message ?: "Could not import the image")
            }
        }
    }
    fun consumed() { if (step.value != Step.Copying) step.value = Step.Idle }
}
