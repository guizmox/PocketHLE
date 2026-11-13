package com.pockethle.app

import android.app.Application
import android.net.Uri
import android.provider.OpenableColumns
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.MutableLiveData
import androidx.lifecycle.viewModelScope
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Keeps prepared data and the pending confirmation across Activity recreation. */
class AddDataModel(application: Application) : AndroidViewModel(application) {
    sealed class Step {
        data object Idle : Step()
        data object Busy : Step()
        data class Zip(val uri: Uri, val name: String, val gameId: String) : Step()
        data class Executable(val paths: List<String>) : Step()
        data class Overwrite(val paths: List<String>) : Step()
        data class Notice(val success: Boolean, val message: String, val launchOptions: JSONObject? = null) : Step()
    }

    val step = MutableLiveData<Step>(Step.Idle)
    private val root = LibraryPaths.root(application)
    private var conflicts: List<String> = emptyList()
    private var executables: List<String> = emptyList()
    private var executable = ""
    @Volatile private var token: Long? = null
    @Volatile private var cleared = false

    fun select(uri: Uri, gameId: String) {
        if (step.value != Step.Idle) return
        step.value = Step.Busy
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val ctx = getApplication<Application>()
                    val name = ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(it.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME)) else null
                    } ?: error("Could not read the selected filename")
                    require(name.isNotBlank()) { "Missing filename" }
                    name
                }
            }
            result.fold(onSuccess = { name ->
                if (name.endsWith(".zip", ignoreCase = true)) step.value = Step.Zip(uri, name, gameId)
                else prepare(uri, name, gameId, false)
            }, onFailure = { notice(it) })
        }
    }

    fun chooseZip(extract: Boolean) {
        val choice = step.value as? Step.Zip ?: return
        prepare(choice.uri, choice.name, choice.gameId, extract)
    }

    private fun prepare(uri: Uri, name: String, gameId: String, extract: Boolean) {
        step.value = Step.Busy
        viewModelScope.launch {
            // Finish native staging even if the Activity is closed, then release
            // its token. A cancelled coroutine must not leak a temporary tree.
            val result = withContext(Dispatchers.IO + NonCancellable) {
                runCatching {
                    val ctx = getApplication<Application>()
                    val cache = File.createTempFile("add-data-", ".tmp", ctx.cacheDir)
                    try {
                        ctx.contentResolver.openInputStream(uri)?.use { input ->
                            cache.outputStream().use { input.copyTo(it) }
                        } ?: error("Could not open the selected file")
                        val json = checked(NativeBridge.prepareGameData(root, gameId, cache.absolutePath, name, extract))
                        val prepared = json.getString("token").toLong()
                        token = prepared
                        if (cleared) { NativeBridge.cancelGameData(root, prepared); token = null }
                        json
                    } finally { cache.delete() }
                }
            }
            result.fold(onSuccess = { json ->
                fun strings(key: String): List<String> {
                    val array = json.getJSONArray(key)
                    return List(array.length()) { array.getString(it) }
                }
                conflicts = strings("conflicts")
                executables = strings("executables")
                executable = ""
                if (executables.isNotEmpty()) step.value = Step.Executable(executables)
                else confirmData()
            }, onFailure = { notice(it) })
        }
    }

    fun chooseExecutable(path: String?) {
        if (step.value !is Step.Executable || (path != null && path !in executables)) return
        executable = path.orEmpty()
        confirmData()
    }

    private fun confirmData() {
        if (conflicts.isEmpty()) commit(false)
        else step.value = Step.Overwrite(conflicts)
    }

    fun confirmOverwrite() {
        if (step.value is Step.Overwrite) commit(true)
    }

    private fun commit(overwrite: Boolean) {
        val prepared = token ?: return
        token = null
        step.value = Step.Busy
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO + NonCancellable) {
                runCatching { checked(NativeBridge.commitGameData(root, prepared, overwrite, executable)) }
            }
            result.fold(onSuccess = { data ->
                val destination = data.getString("destination")
                step.value = Step.Notice(
                    true,
                    "Data added successfully.\n\nFiles were placed in:\n$destination",
                    data.optJSONObject("launch_options")
                )
            }, onFailure = { notice(it) })
        }
    }

    fun cancel() {
        if (step.value == Step.Busy) return
        token?.let { NativeBridge.cancelGameData(root, it) }
        token = null
        conflicts = emptyList()
        executables = emptyList()
        executable = ""
        step.value = Step.Idle
    }

    private fun checked(raw: String): JSONObject = JSONObject(raw).also {
        check(it.optBoolean("ok", false)) { it.optString("error", "Could not add data") }
    }

    private fun notice(error: Throwable) {
        step.value = Step.Notice(false, error.message ?: "Could not add data")
    }

    override fun onCleared() {
        cleared = true
        token?.let { NativeBridge.cancelGameData(root, it) }
        token = null
        super.onCleared()
    }
}
