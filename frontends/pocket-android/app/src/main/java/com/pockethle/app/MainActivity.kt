package com.pockethle.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.text.method.LinkMovementMethod
import android.text.util.Linkify
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.widget.Toolbar
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.GridLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.google.android.material.snackbar.Snackbar
import kotlinx.coroutines.Job
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * Library screen — heavily inspired by
 * [j2me-loader](https://github.com/nikita36078/j2me-loader): a
 * RecyclerView of game cards, a "+" floating action button to import a
 * new `.CAB`, and per-card overflow menu (Run / Settings / Remove).
 */
class MainActivity : HostKeyActivity() {

    private lateinit var addDataModel: AddDataModel
    private var dataDialog: AddDataDialog? = null
    private val addDataPicker = registerForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        val id = pendingDataGameId
        pendingDataGameId = null
        if (uri != null && id != null) addDataModel.select(uri, id)
    }

    private var pendingDataGameId: String? = null

    private fun pickData(entry: GameEntry) {
        if (!libraryBusy && !firstImportActive && addDataModel.step.value == AddDataModel.Step.Idle) {
            pendingDataGameId = entry.id
            addDataPicker.launch(arrayOf("*/*"))
        }
    }

    private fun showDataStep(step: AddDataModel.Step) {
        dataDialog?.dismiss()
        dataDialog = null
        if (step == AddDataModel.Step.Idle) {
            refreshLibrary()
            showNextImportOptions()
            showReleaseUpdate()
            return
        }
        if (step is AddDataModel.Step.Notice && step.success && step.launchOptions != null) {
            importOptionsQueue.addLast(step.launchOptions)
            addDataModel.cancel()
            return
        }
        val dialog = AddDataDialog(this, ::handleHostKeyEvent)
        dataDialog = dialog
        dialog.setCanceledOnTouchOutside(false)
        when (step) {
            AddDataModel.Step.Busy -> {
                dialog.setTitle("Add/Replace Data")
                dialog.setMessage("Please wait…")
                dialog.setCancelable(false)
                dialog.controllerCancel = false
            }
            is AddDataModel.Step.Zip -> {
                dialog.setTitle("Add/Replace Data — ZIP archive")
                dialog.setMessage("Extract the ZIP contents into the game folder, or copy the ZIP file unchanged?")
                dialog.setButton(android.content.DialogInterface.BUTTON_POSITIVE, "Extract contents") { _, _ -> addDataModel.chooseZip(true) }
                dialog.setButton(android.content.DialogInterface.BUTTON_NEUTRAL, "Copy ZIP unchanged") { _, _ -> addDataModel.chooseZip(false) }
                dialog.setButton(android.content.DialogInterface.BUTTON_NEGATIVE, "Cancel") { _, _ -> addDataModel.cancel() }
            }
            is AddDataModel.Step.Executable -> {
                dialog.setTitle("Choose game executable")
                val choices = listOf("Keep current executable") + step.paths
                val list = android.widget.ListView(this).apply {
                    adapter = android.widget.ArrayAdapter(this@MainActivity, android.R.layout.simple_list_item_1, choices)
                    setOnItemClickListener { _, _, position, _ -> addDataModel.chooseExecutable(if (position == 0) null else step.paths[position - 1]) }
                }
                val container = android.widget.FrameLayout(this).apply {
                    addView(list, android.widget.FrameLayout.LayoutParams(android.view.ViewGroup.LayoutParams.MATCH_PARENT, (resources.displayMetrics.heightPixels * 0.45f).toInt()))
                }
                dialog.setView(container)
                dialog.setButton(android.content.DialogInterface.BUTTON_NEGATIVE, "Cancel") { _, _ -> addDataModel.cancel() }
                dialog.setOnShowListener { list.requestFocus(); list.setSelection(0) }
            }
            is AddDataModel.Step.Overwrite -> {
                dialog.setTitle("Replace existing data?")
                dialog.setMessage("Existing files or folders will be replaced. No files have been copied into the game folder yet.\n\n" + step.paths.joinToString("\n"))
                dialog.setButton(android.content.DialogInterface.BUTTON_POSITIVE, "Yes") { _, _ -> addDataModel.confirmOverwrite() }
                dialog.setButton(android.content.DialogInterface.BUTTON_NEGATIVE, "Cancel") { _, _ -> addDataModel.cancel() }
            }
            is AddDataModel.Step.Notice -> {
                dialog.setTitle(if (step.success) "Add/Replace Data complete" else "Add/Replace Data failed")
                dialog.setMessage(step.message)
                dialog.setButton(android.content.DialogInterface.BUTTON_POSITIVE, "Close") { _, _ -> addDataModel.cancel() }
            }
            AddDataModel.Step.Idle -> return
        }
        dialog.setOnCancelListener { addDataModel.cancel() }
        dialog.show()
        applyHostFullscreen(dialog.window!!)
        dialog.getButton(if (step is AddDataModel.Step.Overwrite) android.content.DialogInterface.BUTTON_NEGATIVE else android.content.DialogInterface.BUTTON_POSITIVE)?.requestFocus()
    }



    private lateinit var adapter: GameAdapter
    private var gizmondoTab = true
    private var libraryGames: List<GameEntry> = emptyList()
    private var libraryLoad: Job? = null
    private var refreshPending = false
    private var libraryBusy = false
    private var pendingReleaseUpdate: ReleaseUpdates.Update? = null
    private var busyDialog: AlertDialog? = null
    private lateinit var recycler: RecyclerView
    private lateinit var emptyState: TextView
    private lateinit var rootDir: String
    private var colorsGpsRequest: DevicePositionRequest? = null
    private var cancelColorsGpsFill: (() -> Unit)? = null
    private var pendingGpsFill: (() -> Unit)? = null
    private val gpsFillPermissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants ->
        val fill = pendingGpsFill
        pendingGpsFill = null
        if (grants.values.any { it }) fill?.invoke()
        else if (fill != null) android.widget.Toast.makeText(this, "Location permission denied", android.widget.Toast.LENGTH_LONG).show()
    }

    override fun onStop() {
        cancelColorsGpsFill?.invoke()
        super.onStop()
    }

    private val firstImportQueue = java.util.ArrayDeque<String>()
    private var firstImportDialog: ImportSetupDialog? = null
    private var firstImportLoading = false
    private var restoredFirstImport: JSONObject? = null
    private val firstImportActive: Boolean
        get() = firstImportQueue.isNotEmpty() || firstImportDialog != null || firstImportLoading

    private val importOptionsQueue = java.util.ArrayDeque<JSONObject>()
    private var importOptionsDialog: AlertDialog? = null
    private var waitingForImportSettings = false
    private val importSettings = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        waitingForImportSettings = false
        showNextImportOptions()
    }

    private val importGame = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) handleImport(uri)
    }

    private val backupPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        if (allowed) saveLibraryBackup()
        else Snackbar.make(recycler, "Downloads permission denied. Backup cancelled.", Snackbar.LENGTH_LONG).show()
    }
    private val restoreBackup = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            AlertDialog.Builder(this).setTitle("Restore library backup")
                .setItems(arrayOf("Merge (no overwrite)", "Merge (overwrite)", "Replace — remove existing files", "Cancel")) { _, mode ->
                    if (mode < 3) runLibraryBackup("Validating and restoring backup…") {
                        LibraryBackup.restore(this@MainActivity, rootDir, uri, mode)
                        "Library restored."
                    }
                }.setNegativeButton(android.R.string.cancel, null).show()
        }
    }

    private fun saveLibraryBackup() {
        if (android.os.Build.VERSION.SDK_INT < 29 &&
            androidx.core.content.ContextCompat.checkSelfPermission(this, android.Manifest.permission.WRITE_EXTERNAL_STORAGE) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
            backupPermission.launch(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)
            return
        }
        runLibraryBackup("Saving library backup to Downloads…") {
            "Backup saved to Downloads/" + LibraryBackup.save(this@MainActivity, rootDir)
        }
    }

    private fun runLibraryBackup(message: String, operation: () -> String) {
        if (libraryBusy) return
        setLibraryBusy(message)
        lifecycleScope.launch {
            val result = try {
                withContext(Dispatchers.IO) { runCatching { operation() } }
            } finally { setLibraryBusy(null) }
            result.onSuccess {
                refreshLibrary()
                AlertDialog.Builder(this@MainActivity).setTitle("PocketHLE-Reborn backup").setMessage(it)
                    .setPositiveButton(android.R.string.ok, null).show()
            }.onFailure {
                AlertDialog.Builder(this@MainActivity).setTitle("Backup/restore failed").setMessage(it.message ?: "Unknown error")
                    .setPositiveButton(android.R.string.ok, null).show()
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        setSupportActionBar(findViewById<Toolbar>(R.id.toolbar))

        rootDir = LibraryPaths.root(this)
        savedInstanceState?.getString("first_import_queue")?.let { raw ->
            runCatching { JSONArray(raw) }.getOrNull()?.let { queue ->
                for (i in 0 until queue.length()) queue.optString(i).takeIf { it in listOf("gizmondo", "pocket_pc") }?.let { firstImportQueue.addLast(it) }
            }
        }
        restoredFirstImport = savedInstanceState?.getString("first_import_draft")?.let { runCatching { JSONObject(it) }.getOrNull() }
        savedInstanceState?.getString("import_options_queue")?.let { raw ->
            runCatching { JSONArray(raw) }.getOrNull()?.let { queue ->
                for (i in 0 until queue.length()) queue.optJSONObject(i)?.let { importOptionsQueue.addLast(it) }
            }
        }
        waitingForImportSettings = savedInstanceState?.getBoolean("waiting_import_settings", false) ?: false
        pendingDataGameId = savedInstanceState?.getString("add_data_game_id")
        addDataModel = androidx.lifecycle.ViewModelProvider(this)[AddDataModel::class.java]
        emptyState = findViewById(R.id.empty_state)
        recycler = findViewById(R.id.recycler)
        adapter = GameAdapter(
            onRun = { entry -> launchGame(entry) },
            onSettings = { entry ->
                startActivity(
                    Intent(this, GameSettingsActivity::class.java)
                        .putExtra(GameSettingsActivity.EXTRA_GAME_ID, entry.id)
                        .putExtra(GameSettingsActivity.EXTRA_GAME_NAME, entry.displayName),
                )
            },
            onAddData = { entry -> pickData(entry) },
            onSwitchExecutable = ::switchLaunchExecutable,
            onRemove = { entry -> confirmRemove(entry) },
            onRename = { entry -> renameGame(entry) },
            onInformation = { entry -> showInformation(entry) },
            libraryRoot = rootDir,
        )
        recycler.layoutManager = GridLayoutManager(this, gridSpanCount())
        recycler.adapter = adapter
        addDataModel.step.observe(this) { showDataStep(it) }
        gizmondoTab = savedInstanceState?.getBoolean("gizmondo_tab", true) ?: true
        val tabs = findViewById<com.google.android.material.tabs.TabLayout>(R.id.platform_tabs)
        tabs.addTab(tabs.newTab().setText("Gizmondo"), gizmondoTab)
        tabs.addTab(tabs.newTab().setText("Pocket PC"), !gizmondoTab)
        tabs.addOnTabSelectedListener(object : com.google.android.material.tabs.TabLayout.OnTabSelectedListener {
            override fun onTabSelected(tab: com.google.android.material.tabs.TabLayout.Tab) { gizmondoTab = tab.position == 0; showLibraryTab() }
            override fun onTabUnselected(tab: com.google.android.material.tabs.TabLayout.Tab) {}
            override fun onTabReselected(tab: com.google.android.material.tabs.TabLayout.Tab) {}
        })

        findViewById<FloatingActionButton>(R.id.fab_import).setOnClickListener {
            if (!libraryBusy && !firstImportActive) importGame.launch(arrayOf("application/vnd.ms-cab-compressed", "application/x-rar-compressed", "application/zip", "application/octet-stream", "*/*"))
        }
        lifecycleScope.launch {
            pendingReleaseUpdate = withContext(Dispatchers.IO) {
                ReleaseUpdates.check(BuildConfig.UPDATE_VERSION)
            }
            showReleaseUpdate()
        }
    }

    private fun switchLaunchExecutable(entry: GameEntry) {
        if (libraryBusy) return
        lifecycleScope.launch {
            val response = withContext(Dispatchers.IO) {
                runCatching { JSONObject(NativeBridge.launchExecutables(rootDir, entry.id, null)) }
            }
            val body = response.getOrNull()
            val entries = body?.optJSONArray("executables")
            if (entries == null || entries.length() == 0) {
                AlertDialog.Builder(this@MainActivity)
                    .setTitle("Switch Launch Executable")
                    .setMessage(body?.optString("error", "No launchable executables found.")
                        ?: response.exceptionOrNull()?.localizedMessage
                        ?: "Could not list executables.")
                    .setPositiveButton(android.R.string.ok, null)
                    .show()
                return@launch
            }
            val paths = Array(entries.length()) { entries.getString(it) }
            val labels = Array(paths.size) { paths[it].removePrefix("extracted/") }
            AlertDialog.Builder(this@MainActivity)
                .setTitle("Switch Launch Executable")
                .setSingleChoiceItems(labels, paths.indexOf(body?.optString("current").orEmpty())) { dialog, index ->
                    dialog.dismiss()
                    lifecycleScope.launch {
                        val result = withContext(Dispatchers.IO) {
                            runCatching {
                                val saved = JSONObject(NativeBridge.launchExecutables(rootDir, entry.id, paths[index]))
                                if (!saved.optBoolean("ok")) error(saved.optString("error"))
                                saved
                            }
                        }
                        result.onSuccess { saved ->
                            refreshLibrary()
                            saved.optJSONObject("launch_options")?.let {
                                importOptionsQueue.addLast(it)
                                showNextImportOptions()
                            }
                        }.onFailure { error ->
                            AlertDialog.Builder(this@MainActivity)
                                .setTitle("Could not switch executable")
                                .setMessage(error.localizedMessage)
                                .setPositiveButton(android.R.string.ok, null)
                                .show()
                        }
                    }
                }
                .setNegativeButton(android.R.string.cancel, null)
                .show()
        }
    }

    override fun onResume() {
        super.onResume()
        if (!libraryBusy) refreshLibrary()
        showNextImportOptions()
        showReleaseUpdate()
    }

    private fun showReleaseUpdate() {
        val update = pendingReleaseUpdate ?: return
        if (ReleaseUpdates.announced || libraryBusy || firstImportActive || importOptionsDialog != null || importOptionsQueue.isNotEmpty() || dataDialog?.isShowing == true || isFinishing || isDestroyed ||
            !lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) return
        ReleaseUpdates.announced = true
        pendingReleaseUpdate = null
        Snackbar.make(recycler, "PocketHLE-Reborn ${update.version} is available", Snackbar.LENGTH_INDEFINITE)
            .setAction("Download") {
                runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(update.url))) }
                    .onFailure { android.widget.Toast.makeText(this, "No browser available", android.widget.Toast.LENGTH_LONG).show() }
            }.show()
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.main_menu, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        if (libraryBusy || firstImportActive) return true
        return when (item.itemId) {
            R.id.action_backup -> { saveLibraryBackup(); true }
            R.id.action_restore -> { restoreBackup.launch(arrayOf("application/zip", "application/octet-stream", "*/*")); true }
            R.id.action_settings -> {
                startActivity(Intent(this, SettingsActivity::class.java))
                true
            }
            R.id.action_about -> {
                AlertDialog.Builder(this)
                    .setTitle(R.string.about_title)
                    .setMessage(NativeBridge.banner())
                    .setPositiveButton(android.R.string.ok, null)
                    .show().also { dialog ->
                        dialog.findViewById<TextView>(android.R.id.message)?.apply {
                            Linkify.addLinks(this, Linkify.WEB_URLS)
                            movementMethod = LinkMovementMethod.getInstance()
                        }
                    }
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    /** Roughly 110dp-wide tiles, like a game launcher grid; at least 2 columns. */
    private fun gridSpanCount(): Int {
        val widthDp = resources.configuration.screenWidthDp
        return (widthDp / 164).coerceAtLeast(2)
    }

    override fun onSaveInstanceState(out: Bundle) {
        out.putString("first_import_queue", JSONArray(firstImportQueue.toList()).toString())
        out.putString("first_import_draft", (firstImportDialog?.snapshot() ?: restoredFirstImport)?.toString())
        out.putString("import_options_queue", JSONArray(importOptionsQueue.toList()).toString())
        out.putBoolean("waiting_import_settings", waitingForImportSettings)
        out.putString("add_data_game_id", pendingDataGameId)
        out.putBoolean("gizmondo_tab", gizmondoTab); super.onSaveInstanceState(out)
    }

    private fun showInformation(entry: GameEntry) {
        lifecycleScope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching { JSONObject(NativeBridge.analyzeGame(rootDir, entry.id)) }
            }
            val metadata = result.getOrNull()
            if (metadata == null || !metadata.optBoolean("ok", true)) {
                Snackbar.make(recycler, metadata?.optString("error") ?: "Could not read game information", Snackbar.LENGTH_LONG).show()
            } else GameInformation.show(this@MainActivity, entry.displayName, metadata)
        }
    }

    private fun renameGame(entry: GameEntry) {
        editGameName(entry.id, entry.displayName, "Rename game")
    }

    private fun editGameName(id: String, detectedName: String, title: String, afterClose: (() -> Unit)? = null) {
        val input = android.widget.EditText(this).apply { setText(detectedName); selectAll(); isSingleLine = true }
        val dialog = AlertDialog.Builder(this).setTitle(title).setView(input)
            .setNegativeButton(android.R.string.cancel,null).setPositiveButton("Save",null).create()
        dialog.setOnShowListener {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                val name=input.text.toString().trim()
                if(name.isBlank()) { input.error="Le nom est obligatoire"; return@setOnClickListener }
                val result=runCatching { JSONObject(NativeBridge.renameGame(rootDir,id,name)) }.getOrNull()
                if(result?.optBoolean("ok",false)==true) { dialog.dismiss(); refreshLibrary() }
                else input.error=result?.optString("error") ?: "Could not save settings"
            }
        }
        dialog.setOnDismissListener { afterClose?.invoke() }
        dialog.show()
    }

    private fun refreshLibrary() {
        // Coalesce refreshes, never re-open the entire catalogue once per card.
        // Resuming still reloads saved settings, while tabs only filter memory.
        if (libraryLoad?.isActive == true) { refreshPending = true; return }
        libraryLoad = lifecycleScope.launch {
            do {
                refreshPending = false
                val result = withContext(Dispatchers.IO) {
                    runCatching {
                        val raw = NativeBridge.listGames(rootDir)
                        if (raw.trimStart().startsWith("{")) {
                            val error = JSONObject(raw)
                            throw IllegalStateException(error.optString("error", "Invalid library response"))
                        }
                        GameEntry.listFromJson(raw)
                    }
                }
                result.onSuccess { games -> libraryGames = games; showLibraryTab() }
                    .onFailure { error ->
                        Snackbar.make(recycler, getString(R.string.error_listing_games, error.message ?: "Unknown error"), Snackbar.LENGTH_LONG).show()
                    }
            } while (refreshPending && !libraryBusy)
        }
    }

    private fun showLibraryTab() {
        val visible = libraryGames.filter { it.isGizmondo == gizmondoTab }
        adapter.submit(visible)
        recycler.visibility = if (visible.isEmpty()) View.GONE else View.VISIBLE
        emptyState.visibility = if (visible.isEmpty()) View.VISIBLE else View.GONE
    }

    private fun setLibraryBusy(message: String?) {
        libraryBusy = message != null
        busyDialog?.dismiss()
        busyDialog = if (message == null) null else AlertDialog.Builder(this)
            .setTitle("Please wait")
            .setMessage(message)
            .setCancelable(false)
            .create().also { it.setCanceledOnTouchOutside(false); it.show() }
        if (!libraryBusy) showReleaseUpdate()
    }

    override fun onDestroy() {
        firstImportDialog?.dispose()
        firstImportDialog = null
        importOptionsDialog?.setOnDismissListener(null)
        importOptionsDialog?.dismiss()
        importOptionsDialog = null
        dataDialog?.dismiss()
        dataDialog = null
        busyDialog?.dismiss()
        busyDialog = null
        pendingImportToken?.let { NativeBridge.cancelPreparedImport(rootDir, it) }
        pendingImportToken = null
        super.onDestroy()
    }

    @Volatile private var pendingImportToken: Long? = null

    private fun showImportError(message:String) {
        if(isFinishing || isDestroyed)return
        AlertDialog.Builder(this).setTitle("Import failed")
            .setMessage(message).setPositiveButton(android.R.string.ok,null).show()
    }

    private fun handleImport(uri: Uri) {
        if (libraryBusy) return
        setLibraryBusy("Importing games…")
        val name = queryDisplayName(uri) ?: "import.cab"
        val snack = Snackbar.make(recycler, R.string.import_in_progress, Snackbar.LENGTH_INDEFINITE)
        snack.show()
        lifecycleScope.launch {
            val result = try {
                withContext(Dispatchers.IO) {
                    runCatching {
                        val cabFile = LibraryPaths.copyUriToCache(this@MainActivity, uri, name)
                        val raw = NativeBridge.prepareImport(rootDir, cabFile.absolutePath)
                        val obj = JSONObject(raw)
                        if (!obj.optBoolean("ok", false)) error(obj.optString("error"))
                        val token = obj.getString("token").toLong()
                        pendingImportToken = token
                        if (isDestroyed || isFinishing) {
                            NativeBridge.cancelPreparedImport(rootDir, token)
                            pendingImportToken = null
                            error("Import cancelled")
                        }
                        raw
                    }
                }
            } finally {
                setLibraryBusy(null)
                snack.dismiss()
            }

            result.fold(
                onSuccess = { raw ->
                    showPreparedImport(raw)
                },
                onFailure = { err ->
                    showImportError(err.message ?: "Unknown error")
                },
            )
        }
    }

    private fun showPreparedImport(raw: String) {
        val obj = JSONObject(raw)
        val token = obj.getString("token").toLong()
        val cabinets = obj.optJSONArray("cabinet_choices")
        if (cabinets != null && cabinets.length() > 0) {
            setLibraryBusy("Waiting for cabinet selection…")
            busyDialog?.dismiss()
            busyDialog = null
            val labels = (0 until cabinets.length()).map {
                val c = cabinets.getJSONObject(it)
                "${c.getString("name")} — ${c.getString("architecture") }"
            }.toTypedArray()
            AlertDialog.Builder(this).setTitle("Choose installer cabinet")
                .setCancelable(false)
                .setItems(labels) { _, which ->
                    setLibraryBusy("Preparing selected cabinet…")
                    lifecycleScope.launch {
                        val result = withContext(Dispatchers.IO) {
                            runCatching {
                                val next = NativeBridge.choosePreparedImportCabinet(rootDir, token, cabinets.getJSONObject(which).getInt("index"))
                                val response = JSONObject(next)
                                if (!response.optBoolean("ok", false)) error(response.optString("error"))
                                next
                            }
                        }
                        setLibraryBusy(null)
                        result.onSuccess { showPreparedImport(it) }.onFailure { error ->
                            NativeBridge.cancelPreparedImport(rootDir, token)
                            pendingImportToken = null
                            showImportError(error.message ?: "Unknown error")
                        }
                    }
                }
                .setNegativeButton("Cancel import") { _, _ ->
                    NativeBridge.cancelPreparedImport(rootDir, token)
                    pendingImportToken = null
                    setLibraryBusy(null)
                }.show()
            return
        }
        val choices = obj.optJSONArray("platform_choices")
        if (choices != null && choices.length() > 0) {
            val game = choices.getJSONObject(0)
            setLibraryBusy("Waiting for platform selection…")
            busyDialog?.dismiss()
            busyDialog = null
            fun choose(gizmondo: Boolean) {
                runCatching {
                    val next = NativeBridge.choosePreparedImportPlatform(rootDir, token, game.getString("id"), gizmondo)
                    val result = JSONObject(next)
                    if (!result.optBoolean("ok", false)) error(result.optString("error"))
                    next
                }.onSuccess { showPreparedImport(it) }.onFailure { error ->
                    NativeBridge.cancelPreparedImport(rootDir, token)
                    pendingImportToken = null
                    setLibraryBusy(null)
                    showImportError(error.message ?: "Unknown error")
                }
            }
            AlertDialog.Builder(this).setTitle("Choose game platform")
                .setMessage("No Gizmondo marker was found for ${game.getString("display_name")}. Which platform is this game for?")
                .setCancelable(false)
                .setPositiveButton("Gizmondo") { _, _ -> choose(true) }
                .setNeutralButton("Pocket PC") { _, _ -> choose(false) }
                .setNegativeButton("Cancel import") { _, _ ->
                    NativeBridge.cancelPreparedImport(rootDir, token)
                    pendingImportToken = null
                    setLibraryBusy(null)
                }.show()
            return
        }
        val executableChoices = obj.optJSONArray("executable_choices")
        if (executableChoices != null && executableChoices.length() > 0) {
            val game = executableChoices.getJSONObject(0)
            val executables = game.getJSONArray("executables")
            setLibraryBusy("Waiting for executable selection…")
            busyDialog?.dismiss()
            busyDialog = null
            val labels = Array(executables.length()) { executables.getString(it).removePrefix("extracted/") }
            fun cancelImport() {
                NativeBridge.cancelPreparedImport(rootDir, token)
                pendingImportToken = null
                setLibraryBusy(null)
            }
            val title = "Choose executable for ${game.getString("display_name")}"
            val hint = "You can change this later in the game’s ⋯ menu → Switch Launch Executable."
            val heading = android.text.SpannableString("$title\n$hint").apply {
                setSpan(android.text.style.RelativeSizeSpan(0.75f), title.length + 1, length, 0)
            }
            AlertDialog.Builder(this)
                .setTitle(heading)
                .setCancelable(false)
                .setItems(labels) { _, index ->
                    runCatching {
                        val next = NativeBridge.choosePreparedImportExecutable(rootDir, token, game.getString("id"), index)
                        val response = JSONObject(next)
                        if (!response.optBoolean("ok", false)) error(response.optString("error"))
                        next
                    }.onSuccess { showPreparedImport(it) }.onFailure { error ->
                        cancelImport()
                        showImportError(error.message ?: "Unknown error")
                    }
                }
                .setNegativeButton("Cancel import") { _, _ -> cancelImport() }
                .show()
            return
        }
        val updates = obj.getJSONArray("updates")
        if (updates.length() == 0) {
            commitPreparedImport(token, false)
            return
        }
        val names = (0 until updates.length()).map { updates.getJSONObject(it).getString("display_name") }
        val newCount = obj.getInt("count") - updates.length()
        setLibraryBusy("Waiting for update confirmation…")
        busyDialog?.dismiss()
        busyDialog = null
        val message = "Already installed:\n" + names.joinToString("\n") +
            "\n\nReplace their game files and keep saves, names and settings?" +
            if (newCount > 0) "\n$newCount new game(s) will also be imported." else ""
        AlertDialog.Builder(this).setTitle("Update existing games?")
            .setMessage(message).setCancelable(false)
            .setNegativeButton("Cancel import") { _, _ ->
                NativeBridge.cancelPreparedImport(rootDir, token)
                pendingImportToken = null
                setLibraryBusy(null)
            }
            .setPositiveButton("Update games") { _, _ -> commitPreparedImport(token, true) }
            .show()
    }

    private fun commitPreparedImport(token: Long, allowUpdates: Boolean) {
        setLibraryBusy(if (allowUpdates) "Updating game files…" else "Importing games…")
        lifecycleScope.launch {
            val result = try {
                withContext(Dispatchers.IO) {
                    runCatching { NativeBridge.commitPreparedImport(rootDir, token, allowUpdates) }
                }
            } finally {
                NativeBridge.cancelPreparedImport(rootDir, token)
                pendingImportToken = null
                setLibraryBusy(null)
            }
            result.fold(onSuccess = { raw ->
                handleImportResult(raw, !allowUpdates)
                refreshLibrary()
            }, onFailure = { err ->
                Snackbar.make(recycler, getString(R.string.import_failed, err.message ?: "unknown"), Snackbar.LENGTH_LONG).show()
            })
        }
    }

    private fun enqueueImportOptions(entries: List<JSONObject>) {
        entries.forEach { entry ->
            entry.optJSONArray("first_import_setup")?.let { platforms ->
                for (i in 0 until platforms.length()) {
                    val platform = platforms.optString(i)
                    if (platform in listOf("gizmondo", "pocket_pc") && !firstImportQueue.contains(platform)) firstImportQueue.addLast(platform)
                }
            }
            entry.optJSONObject("launch_options")?.let { options ->
                if (importOptionsQueue.none { it.optString("id") == options.optString("id") }) {
                    importOptionsQueue.addLast(options.put("display_name", entry.optString("display_name")))
                }
            }
        }
        showNextImportOptions()
    }

    private fun showNextFirstImportSetup() {
        if (firstImportDialog != null || firstImportLoading || libraryBusy || waitingForImportSettings
            || isFinishing || isDestroyed || !lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) return
        val platform = firstImportQueue.peekFirst() ?: return
        firstImportLoading = true
        lifecycleScope.launch {
            val read = withContext(Dispatchers.IO) { runCatching { JSONObject(NativeBridge.readConfig(rootDir)) } }
            firstImportLoading = false
            read.fold(onSuccess = { raw ->
                if (raw.has("error")) {
                    firstImportQueue.pollFirst()
                    Snackbar.make(recycler, raw.optString("error"), Snackbar.LENGTH_LONG).show()
                    showNextImportOptions()
                    return@fold
                }
                if (raw.optJSONObject("first_import_setup")?.optBoolean(platform, false) == true) {
                    firstImportQueue.pollFirst(); restoredFirstImport = null
                    showNextImportOptions(); return@fold
                }
                lateinit var setup: ImportSetupDialog
                setup = ImportSetupDialog(this@MainActivity, platform, LauncherConfig.fromJson(raw),
                    ::handleHostKeyEvent, restoredFirstImport?.takeIf { it.optString("platform") == platform }) { choices ->
                    lifecycleScope.launch {
                        val saved = withContext(Dispatchers.IO) {
                            runCatching {
                                val result = JSONObject(NativeBridge.finishImportSetup(rootDir, choices.toString()))
                                check(result.optBoolean("ok", false)) { result.optString("error", "Could not save settings") }
                            }
                        }
                        saved.fold(onSuccess = {
                            setup.dispose(); firstImportDialog = null
                            firstImportQueue.pollFirst(); restoredFirstImport = null
                            showNextImportOptions()
                        }, onFailure = { setup.failed(it.message ?: "Unknown error") })
                    }
                }
                restoredFirstImport = null
                firstImportDialog = setup
                setup.show()
            }, onFailure = {
                firstImportQueue.pollFirst()
                Snackbar.make(recycler, it.message ?: "Could not read setup settings", Snackbar.LENGTH_LONG).show()
                showNextImportOptions()
            })
        }
    }

    private fun showNextImportOptions() {
        if (firstImportActive) { showNextFirstImportSetup(); return }
        if (isFinishing || isDestroyed || libraryBusy || waitingForImportSettings ||
            importOptionsDialog != null || dataDialog?.isShowing == true) return
        val entry = importOptionsQueue.peekFirst() ?: return
        val id = entry.getString("id")
        val currentProfile = entry.optString("current_profile")
        val profiles = entry.optJSONArray("profiles")
        val message = "${entry.optString("display_name")}\n\n${entry.optString("message")}"
        val displayAdvice = entry.optBoolean("show_display_advice")
        val title = if (displayAdvice) "Pocket PC launch settings" else "GPU compatibility profile"
        val builder = AlertDialog.Builder(this).setTitle(title)
        val choices = android.widget.RadioGroup(this)
        if (profiles != null && profiles.length() > 0) {
            val padding = (24 * resources.displayMetrics.density).toInt()
            choices.setPadding(padding, 0, padding, 0)
            choices.addView(android.widget.TextView(this).apply { text = message })
            for (index in 0 until profiles.length()) {
                val profile = profiles.getJSONObject(index)
                choices.addView(android.widget.RadioButton(this).apply {
                    this.id = android.view.View.generateViewId()
                    text = profile.getString("label")
                    tag = profile.getString("id")
                    isChecked = tag == currentProfile
                })
            }
            builder.setView(android.widget.ScrollView(this).apply { addView(choices) })
        } else {
            builder.setMessage(message)
        }

        fun openSettings() {
            waitingForImportSettings = true
            importSettings.launch(Intent(this, GameSettingsActivity::class.java)
                .putExtra(GameSettingsActivity.EXTRA_GAME_ID, id)
                .putExtra(GameSettingsActivity.EXTRA_GAME_NAME, entry.optString("display_name")))
        }

        fun applyChoice(settings: Boolean) {
            val selected = choices.findViewById<android.view.View>(choices.checkedRadioButtonId)?.tag as? String
            if (selected == null || selected == currentProfile) {
                if (settings) openSettings()
                return
            }
            waitingForImportSettings = true
            lifecycleScope.launch {
                val result = withContext(Dispatchers.IO) {
                    runCatching {
                        val config = JSONObject(NativeBridge.readGameSettings(rootDir, id))
                        check(!config.has("error")) { config.optString("error") }
                        config.put("gpu_compatibility_profile", selected)
                        val saved = JSONObject(NativeBridge.writeGameSettings(rootDir, id, config.toString()))
                        check(saved.optBoolean("ok")) { saved.optString("error") }
                    }
                }
                waitingForImportSettings = false
                result.onSuccess {
                    refreshLibrary()
                    if (settings) openSettings() else showNextImportOptions()
                }.onFailure { error ->
                    importOptionsQueue.addFirst(entry)
                    AlertDialog.Builder(this@MainActivity)
                        .setTitle("Could not save GPU profile")
                        .setMessage(error.localizedMessage)
                        .setPositiveButton(android.R.string.ok, null)
                        .setOnDismissListener { showNextImportOptions() }
                        .show()
                }
            }
        }

        builder.setPositiveButton("Continue") { _, _ -> applyChoice(false) }
        if (displayAdvice) {
            builder.setNegativeButton("Open game settings") { _, _ -> applyChoice(true) }
        }
        val dialog = builder.create()
        importOptionsDialog = dialog
        dialog.setOnDismissListener {
            importOptionsQueue.pollFirst()
            importOptionsDialog = null
            recycler.post { showNextImportOptions() }
        }
        dialog.show()
        val continueButton = dialog.getButton(android.content.DialogInterface.BUTTON_POSITIVE)
        choices.setOnCheckedChangeListener { group, checkedId ->
            val profile = group.findViewById<android.view.View>(checkedId)?.tag
            continueButton.text = if (profile != null && profile != currentProfile) "Save profile" else "Continue"
        }
        choices.findViewById<android.view.View>(choices.checkedRadioButtonId)?.requestFocus()
            ?: dialog.getButton(android.content.DialogInterface.BUTTON_POSITIVE).requestFocus()
    }

    private fun handleImportResult(raw: String, confirmName: Boolean = true) {
        try {
            if (raw.trimStart().startsWith("[")) {
                val entries = GameEntry.listFromJson(raw)
                val imported = JSONArray(raw)
                enqueueImportOptions((0 until imported.length()).map { imported.getJSONObject(it) })
                gizmondoTab = entries.firstOrNull()?.isGizmondo ?: true
                findViewById<com.google.android.material.tabs.TabLayout>(R.id.platform_tabs).getTabAt(if (gizmondoTab) 0 else 1)?.select()
                Snackbar.make(recycler, if (confirmName) "Imported ${entries.size} separate games." else "Imported or updated ${entries.size} games. Saves and settings kept.", Snackbar.LENGTH_LONG).show()
                return
            }
            val obj = JSONObject(raw)
            if (!obj.optBoolean("ok", true)) {
                showImportError(obj.optString("error"))
                return
            }
            val name = obj.optString("display_name", obj.optString("id", "?"))
            val id = obj.getString("id")
            if (confirmName && NativeBridge.isDemoCard(rootDir,id)) {
                AlertDialog.Builder(this).setTitle("Import demo card")
                    .setMessage("Keep the original launcher or create a separate directory and library entry for each game?")
                    .setCancelable(false)
                    .setNegativeButton("Keep launcher") { _,_ -> editGameName(id,name,"Confirm game name") { enqueueImportOptions(listOf(obj)) } }
                    .setPositiveButton("Separate games") { _,_ ->
                        setLibraryBusy("Separating demo card games…")
                        lifecycleScope.launch {
                            val split = try {
                                withContext(Dispatchers.IO) {
                                    runCatching { GameEntry.listFromJson(NativeBridge.splitDemoCard(rootDir, id)) }
                                }
                            } finally {
                                setLibraryBusy(null)
                            }
                            split.fold(onSuccess={ entries ->
                                refreshLibrary()
                                enqueueImportOptions(listOf(obj))
                                Snackbar.make(recycler,"Imported ${entries.size} separate games. Names can be edited from each game menu.",Snackbar.LENGTH_LONG).show()
                            },onFailure={ e ->
                                enqueueImportOptions(listOf(obj))
                                Snackbar.make(recycler,"Could not split card: ${e.message}",Snackbar.LENGTH_LONG).show()
                            })
                        }
                    }.show()
            } else if (confirmName && obj.optBoolean("is_gizmondo")) editGameName(id, name, "Confirm game name") {
                enqueueImportOptions(listOf(obj))
            } else enqueueImportOptions(listOf(obj))
            Snackbar.make(
                recycler,
                if (confirmName) getString(R.string.import_success, name) else "Updated $name; saves and settings kept.",
                Snackbar.LENGTH_SHORT,
            ).show()
        } catch (e: Exception) {
            Snackbar.make(recycler, raw, Snackbar.LENGTH_LONG).show()
        }
    }

    private fun queryDisplayName(uri: Uri): String? {
        return contentResolver.query(uri, null, null, null, null)?.use { c ->
            val ix = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (ix >= 0 && c.moveToFirst()) c.getString(ix) else null
        }
    }

    private fun confirmRemove(entry: GameEntry) {
        AlertDialog.Builder(this)
            .setTitle(R.string.remove_title)
            .setMessage(getString(R.string.remove_message, entry.displayName))
            .setPositiveButton(R.string.remove) { _, _ ->
                val raw = NativeBridge.removeGame(rootDir, entry.id)
                val obj = runCatching { JSONObject(raw) }.getOrNull()
                if (obj?.optBoolean("ok") == true) {
                    refreshLibrary()
                } else {
                    Snackbar.make(
                        recycler,
                        obj?.optString("error") ?: raw,
                        Snackbar.LENGTH_LONG,
                    ).show()
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private var launchPending = false

    private fun launchGame(entry: GameEntry) {
        if (libraryBusy || firstImportActive || launchPending) return
        launchPending = true
        lifecycleScope.launch {
            try {
                val result = withContext(Dispatchers.IO) {
                    Triple(NativeBridge.isColorsGame(rootDir, entry.id),
                        JSONArray(NativeBridge.colorsLanguages(rootDir, entry.id)),
                        JSONObject(NativeBridge.readConfig(rootDir)))
                }
                if (!result.first) {
                    launchPending = false
                    startGame(entry)
                    return@launch
                }
                showColorsStartup(entry, result.second, result.third)
            } catch (error: Exception) {
                launchPending = false
                Snackbar.make(recycler, error.message ?: "Could not read launch options", Snackbar.LENGTH_LONG).show()
            }
        }
    }

    private fun showColorsStartup(entry: GameEntry, languages: JSONArray, config: JSONObject) {
        lateinit var dialog: AlertDialog
        val content = android.widget.LinearLayout(this).apply {
            orientation = android.widget.LinearLayout.VERTICAL
            val padding = (20 * resources.displayMetrics.density).toInt()
            setPadding(padding, padding, padding, padding)
        }
        fun label(text: String): TextView = TextView(this).apply { this.text = text }.also { content.addView(it) }
        fun spinner(labels: List<String>) = android.widget.Spinner(this).also {
            it.adapter = android.widget.ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, labels)
            content.addView(it)
        }
        fun field(title: String, value: String, coordinates: Boolean = false, sectionTitle: Boolean = false): android.widget.EditText {
            val heading = label(title)
            if (sectionTitle) {
                heading.setTypeface(heading.typeface, android.graphics.Typeface.BOLD)
                heading.textSize = 18f
                heading.setPadding(0, 0, 0, (8 * resources.displayMetrics.density).toInt())
            }
            return android.widget.EditText(this).also {
                it.setText(value)
                it.setSingleLine(true)
                if (coordinates) it.inputType = android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL or android.text.InputType.TYPE_NUMBER_FLAG_SIGNED
                content.addView(it)
            }
        }
        val ids = (0 until languages.length()).map { languages.getJSONObject(it).getString("id") }
        val labels = (0 until languages.length()).map { languages.getJSONObject(it).getString("label") }
        val language = if (ids.size > 1) {
            label("Language")
            spinner(labels).apply {
                val previous = ids.indexOf(config.optString("colors_language", "english"))
                setSelection(if (previous >= 0) previous else ids.indexOf("english").coerceAtLeast(0))
            }
        } else {
            label("Language: ${labels.firstOrNull() ?: "English"}")
            null
        }
        val fakeGps = androidx.appcompat.widget.SwitchCompat(this).apply {
            text = "Fake GPS"
            isChecked = false
        }
        content.addView(fakeGps)
        val mode = spinner(listOf("Near Eiffel Tower (within 800 m)", "Manual coordinates"))
        mode.setSelection(if (config.optBoolean("gps_fixed_random")) 0 else 1)
        val manualGpsStart = content.childCount
        val latitude = field("Latitude", config.optDouble("gps_fixed_latitude", 0.0).toString(), true)
        val longitude = field("Longitude", config.optDouble("gps_fixed_longitude", 0.0).toString(), true)
        label("Decimal coordinates: north/east positive, south/west negative.")
        label("Fake GPS replaces device location for this launch.")
        val fillStatus = TextView(this)
        val fillGps = android.widget.Button(this).apply { text = "Fill GPS" }
        content.addView(fillGps)
        content.addView(fillStatus)
        val manualGpsViews = (manualGpsStart until content.childCount).map { content.getChildAt(it) }
        fillStatus.text = "Copy the latest known position, or request a new fix."
        fun fillDevicePosition() {
            if (!dialog.isShowing || colorsGpsRequest != null) return
            val permissions = GpsHost.permissions()
            val granted = permissions.any {
                androidx.core.content.ContextCompat.checkSelfPermission(this, it) == android.content.pm.PackageManager.PERMISSION_GRANTED
            }
            if (!granted) {
                pendingGpsFill = { fillDevicePosition() }
                gpsFillPermissions.launch(permissions)
                return
            }
            fillGps.isEnabled = false
            fillStatus.text = "Waiting for device position…"
            colorsGpsRequest = DevicePositionRequest(this) { location, message ->
                colorsGpsRequest = null
                if (dialog.isShowing) {
                    fillGps.isEnabled = true
                    if (location != null) {
                        latitude.setText(location.latitude.toString())
                        longitude.setText(location.longitude.toString())
                        val timestamp = java.text.DateFormat.getDateTimeInstance().format(java.util.Date(location.time))
                        fillStatus.text = "Coordinates copied (position: $timestamp)"
                        // Filling coordinates never enables Fake GPS or saves the draft.
                    } else {
                        fillStatus.text = message ?: "No position available"
                    }
                }
            }
            colorsGpsRequest?.start()
        }
        cancelColorsGpsFill = {
            if (colorsGpsRequest != null) {
                colorsGpsRequest?.close()
                colorsGpsRequest = null
                fillGps.isEnabled = true
                fillStatus.text = "Position request canceled. Tap Fill GPS to try again."
            }
        }
        fillGps.setOnClickListener { fillDevicePosition() }
        fun updateGpsControls() {
            mode.isEnabled = fakeGps.isChecked
            val manual = fakeGps.isChecked && mode.selectedItemPosition == 1
            manualGpsViews.forEach { it.visibility = if (manual) View.VISIBLE else View.GONE }
            latitude.isEnabled = manual
            longitude.isEnabled = manual
            if (!manual) cancelColorsGpsFill?.invoke()
        }
        fakeGps.setOnCheckedChangeListener { _, _ -> updateGpsControls() }
        mode.onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: android.widget.AdapterView<*>?, view: View?, position: Int, id: Long) { updateGpsControls() }
            override fun onNothingSelected(parent: android.widget.AdapterView<*>?) {}
        }
        updateGpsControls()
        content.addView(View(this).apply { setBackgroundColor(android.graphics.Color.GRAY) }, android.widget.LinearLayout.LayoutParams(
            android.widget.LinearLayout.LayoutParams.MATCH_PARENT,
            (1 * resources.displayMetrics.density).toInt().coerceAtLeast(1),
        ).apply {
            topMargin = (20 * resources.displayMetrics.density).toInt()
            bottomMargin = (16 * resources.displayMetrics.density).toInt()
        })
        val server = field("Colors multiplayer server", config.optString("colors_server_url"), sectionTitle = true)
        label("Host, IP or HTTP(S) origin. Leave empty to use the public Colors server.")
        val unlockTurf = android.widget.Button(this).apply {
            text = "Unlock Turf Wars + Tag Editor"
        }
        content.addView(unlockTurf)
        unlockTurf.setOnClickListener {
            unlockTurf.isEnabled = false
            dialog.setCancelable(false)
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = false
            dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = false
            unlockTurf.text = "Unlocking Turf Wars + Tag Editor…"
            lifecycleScope.launch {
                try {
                    val result = withContext(Dispatchers.IO) {
                        JSONObject(NativeBridge.unlockColorsTurfWars(rootDir, entry.id))
                    }
                    if (dialog.isShowing) {
                        val message = if (result.optBoolean("ok")) {
                            result.optString("message")
                        } else result.optString("error", "Could not unlock Colors menus")
                        AlertDialog.Builder(this@MainActivity).setTitle("Unlock Turf Wars + Tag Editor")
                            .setMessage(message).setPositiveButton(android.R.string.ok, null).show()
                    }
                } catch (error: Exception) {
                    if (dialog.isShowing) AlertDialog.Builder(this@MainActivity).setTitle("Unlock Turf Wars + Tag Editor")
                        .setMessage(error.message ?: "Could not unlock Colors menus")
                        .setPositiveButton(android.R.string.ok, null).show()
                } finally {
                    if (dialog.isShowing) {
                        unlockTurf.text = "Unlock Turf Wars + Tag Editor"
                        unlockTurf.isEnabled = true
                        dialog.setCancelable(true)
                        dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true
                        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = true
                    }
                }
            }
        }
        dialog = AlertDialog.Builder(this)
            .setTitle("Colors startup")
            .setView(android.widget.ScrollView(this).apply { addView(content) })
            .setPositiveButton("Start", null)
            .setNegativeButton(android.R.string.cancel, null)
            .create()
        dialog.setOnDismissListener {
            cancelColorsGpsFill?.invoke()
            cancelColorsGpsFill = null
            pendingGpsFill = null
            launchPending = false
        }
        dialog.setOnShowListener {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                pendingGpsFill = null
                cancelColorsGpsFill?.invoke()
                val lat = latitude.text.toString().trim().replace(',', '.').toDoubleOrNull()
                val lon = longitude.text.toString().trim().replace(',', '.').toDoubleOrNull()
                if (fakeGps.isChecked && mode.selectedItemPosition == 1 &&
                    (lat == null || lon == null || !lat.isFinite() || !lon.isFinite() || lat !in -90.0..90.0 || lon !in -180.0..180.0)) {
                    android.widget.Toast.makeText(this, "Invalid coordinates: latitude −90..90, longitude −180..180", android.widget.Toast.LENGTH_LONG).show()
                    return@setOnClickListener
                }
                val input = server.text.toString().trim()
                val origin = if (input.isEmpty() || input.contains("://")) input else "http://$input"
                val uri = android.net.Uri.parse(origin)
                if (origin.isNotEmpty() && (uri.scheme !in listOf("http", "https") || uri.host.isNullOrBlank() || !uri.userInfo.isNullOrBlank() || !uri.query.isNullOrBlank() || !uri.fragment.isNullOrBlank() || (uri.path ?: "") !in listOf("", "/"))) {
                    server.error = "Enter a host or HTTP(S) origin without a path"
                    return@setOnClickListener
                }
                val selected = language?.let { ids[it.selectedItemPosition] } ?: ids.firstOrNull() ?: "english"
                val fixedEnabled = fakeGps.isChecked
                val fixedRandom = mode.selectedItemPosition == 0
                unlockTurf.isEnabled = false
                dialog.setCancelable(false)
                dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = false
                dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = false
                lifecycleScope.launch {
                    try {
                        val saved = withContext(Dispatchers.IO) {
                            val latest = JSONObject(NativeBridge.readConfig(rootDir))
                            latest.put("colors_language", selected)
                            latest.put("gps_fixed_enabled", fixedEnabled)
                            latest.put("gps_fixed_random", fixedRandom)
                            if (lat != null && lat.isFinite() && lat in -90.0..90.0) latest.put("gps_fixed_latitude", lat)
                            if (lon != null && lon.isFinite() && lon in -180.0..180.0) latest.put("gps_fixed_longitude", lon)
                            latest.put("colors_server_url", origin.trimEnd('/'))
                            JSONObject(NativeBridge.writeConfig(rootDir, latest.toString()))
                        }
                        if (saved.optBoolean("ok")) { dialog.dismiss(); startGame(entry) }
                        else {
                            unlockTurf.isEnabled = true
                            dialog.setCancelable(true)
                            dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = true
                            dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true
                            Snackbar.make(recycler, saved.optString("error", "Could not save options"), Snackbar.LENGTH_LONG).show()
                        }
                    } catch (error: Exception) {
                        unlockTurf.isEnabled = true
                        dialog.setCancelable(true)
                        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).isEnabled = true
                        dialog.getButton(AlertDialog.BUTTON_POSITIVE).isEnabled = true
                        Snackbar.make(recycler, error.message ?: "Could not save options", Snackbar.LENGTH_LONG).show()
                    }
                }
            }
        }
        dialog.show()
    }

    private fun startGame(entry: GameEntry) {
        if (libraryBusy || firstImportActive) return
        startActivity(
            Intent(this, GameActivity::class.java)
                .putExtra(GameActivity.EXTRA_GAME_ID, entry.id)
                .putExtra(GameActivity.EXTRA_GAME_NAME, entry.displayName),
        )
    }
}
