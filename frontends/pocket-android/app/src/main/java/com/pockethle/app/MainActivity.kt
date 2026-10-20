package com.pockethle.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.widget.TextView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.GridLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.FloatingActionButton
import com.google.android.material.snackbar.Snackbar
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
class MainActivity : AppCompatActivity() {

    private lateinit var adapter: GameAdapter
    private var gizmondoTab = true
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

    private val importGame = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) handleImport(uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)
        setSupportActionBar(findViewById<Toolbar>(R.id.toolbar))

        rootDir = LibraryPaths.root(this)
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
            onRemove = { entry -> confirmRemove(entry) },
            onRename = { entry -> renameGame(entry) },
            onInformation = { entry -> showInformation(entry) },
            libraryRoot = rootDir,
        )
        recycler.layoutManager = GridLayoutManager(this, gridSpanCount())
        recycler.adapter = adapter
        gizmondoTab = savedInstanceState?.getBoolean("gizmondo_tab", true) ?: true
        val tabs = findViewById<com.google.android.material.tabs.TabLayout>(R.id.platform_tabs)
        tabs.addTab(tabs.newTab().setText("Gizmondo"), gizmondoTab)
        tabs.addTab(tabs.newTab().setText("Pocket PC"), !gizmondoTab)
        tabs.addOnTabSelectedListener(object : com.google.android.material.tabs.TabLayout.OnTabSelectedListener {
            override fun onTabSelected(tab: com.google.android.material.tabs.TabLayout.Tab) { gizmondoTab = tab.position == 0; refreshLibrary() }
            override fun onTabUnselected(tab: com.google.android.material.tabs.TabLayout.Tab) {}
            override fun onTabReselected(tab: com.google.android.material.tabs.TabLayout.Tab) {}
        })

        findViewById<FloatingActionButton>(R.id.fab_import).setOnClickListener {
            if (!libraryBusy) importGame.launch(arrayOf("application/vnd.ms-cab-compressed", "application/x-rar-compressed", "application/zip", "application/octet-stream", "*/*"))
        }
        lifecycleScope.launch {
            pendingReleaseUpdate = withContext(Dispatchers.IO) {
                ReleaseUpdates.check(BuildConfig.UPDATE_VERSION)
            }
            showReleaseUpdate()
        }
    }

    override fun onResume() {
        super.onResume()
        if (!libraryBusy) refreshLibrary()
        showReleaseUpdate()
    }

    private fun showReleaseUpdate() {
        val update = pendingReleaseUpdate ?: return
        if (ReleaseUpdates.announced || libraryBusy || isFinishing || isDestroyed ||
            !lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) return
        ReleaseUpdates.announced = true
        pendingReleaseUpdate = null
        Snackbar.make(recycler, "PocketHLE ${update.version} is available", Snackbar.LENGTH_INDEFINITE)
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
        if (libraryBusy) return true
        return when (item.itemId) {
            R.id.action_settings -> {
                startActivity(Intent(this, SettingsActivity::class.java))
                true
            }
            R.id.action_about -> {
                AlertDialog.Builder(this)
                    .setTitle(R.string.about_title)
                    .setMessage(NativeBridge.banner())
                    .setPositiveButton(android.R.string.ok, null)
                    .show()
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

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putBoolean("gizmondo_tab", gizmondoTab); super.onSaveInstanceState(outState)
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

    private fun editGameName(id: String, detectedName: String, title: String) {
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
        dialog.show()
    }

    private fun refreshLibrary() {
        val raw = NativeBridge.listGames(rootDir)
        val parsed = parseGamesOrToast(raw).filter { NativeBridge.isGizmondoGame(rootDir,it.id) == gizmondoTab }
        adapter.submit(parsed)
        if (parsed.isEmpty()) {
            recycler.visibility = View.GONE
            emptyState.visibility = View.VISIBLE
        } else {
            recycler.visibility = View.VISIBLE
            emptyState.visibility = View.GONE
        }
    }

    private fun parseGamesOrToast(raw: String): List<GameEntry> {
        return try {
            // Library returns [] when empty, JSON object {ok:false,error:...} on error.
            if (raw.startsWith("{")) {
                val obj = JSONObject(raw)
                if (!obj.optBoolean("ok", true)) {
                    Snackbar.make(
                        recycler,
                        getString(R.string.error_listing_games, obj.optString("error")),
                        Snackbar.LENGTH_LONG,
                    ).show()
                }
                emptyList()
            } else {
                JSONArray(raw)
                GameEntry.listFromJson(raw)
            }
        } catch (e: Exception) {
            Snackbar.make(recycler, "Library parse failed: ${e.message}", Snackbar.LENGTH_LONG).show()
            emptyList()
        }
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
        busyDialog?.dismiss()
        busyDialog = null
        pendingImportToken?.let { NativeBridge.cancelPreparedImport(rootDir, it) }
        pendingImportToken = null
        super.onDestroy()
    }

    @Volatile private var pendingImportToken: Long? = null

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
                    Snackbar.make(
                        recycler,
                        getString(R.string.import_failed, err.message ?: "unknown"),
                        Snackbar.LENGTH_LONG,
                    ).show()
                },
            )
        }
    }

    private fun showPreparedImport(raw: String) {
        val obj = JSONObject(raw)
        val token = obj.getString("token").toLong()
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
                    Snackbar.make(recycler, error.message ?: "Import failed", Snackbar.LENGTH_LONG).show()
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

    private fun handleImportResult(raw: String, confirmName: Boolean = true) {
        try {
            if (raw.trimStart().startsWith("[")) {
                val entries = GameEntry.listFromJson(raw)
                gizmondoTab = true
                findViewById<com.google.android.material.tabs.TabLayout>(R.id.platform_tabs).getTabAt(0)?.select()
                Snackbar.make(recycler, if (confirmName) "Imported ${entries.size} separate games." else "Imported or updated ${entries.size} games. Saves and settings kept.", Snackbar.LENGTH_LONG).show()
                return
            }
            val obj = JSONObject(raw)
            if (!obj.optBoolean("ok", true)) {
                Snackbar.make(
                    recycler,
                    getString(R.string.import_failed, obj.optString("error")),
                    Snackbar.LENGTH_LONG,
                ).show()
                return
            }
            val name = obj.optString("display_name", obj.optString("id", "?"))
            val id = obj.getString("id")
            if (confirmName && NativeBridge.isDemoCard(rootDir,id)) {
                AlertDialog.Builder(this).setTitle("Import demo card")
                    .setMessage("Keep the original launcher or create a separate directory and library entry for each game?")
                    .setCancelable(false)
                    .setNegativeButton("Keep launcher") { _,_ -> editGameName(id,name,"Confirm game name") }
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
                                Snackbar.make(recycler,"Imported ${entries.size} separate games. Names can be edited from each game menu.",Snackbar.LENGTH_LONG).show()
                            },onFailure={ e -> Snackbar.make(recycler,"Could not split card: ${e.message}",Snackbar.LENGTH_LONG).show() })
                        }
                    }.show()
            } else if (confirmName) editGameName(id, name, "Confirm game name")
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
        if (libraryBusy || launchPending) return
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
        startActivity(
            Intent(this, GameActivity::class.java)
                .putExtra(GameActivity.EXTRA_GAME_ID, entry.id)
                .putExtra(GameActivity.EXTRA_GAME_NAME, entry.displayName),
        )
    }
}
