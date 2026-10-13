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
    private var busyDialog: AlertDialog? = null
    private lateinit var recycler: RecyclerView
    private lateinit var emptyState: TextView
    private lateinit var rootDir: String

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
    }

    override fun onResume() {
        super.onResume()
        if (!libraryBusy) refreshLibrary()
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

    private fun launchGame(entry: GameEntry) {
        startActivity(
            Intent(this, GameActivity::class.java)
                .putExtra(GameActivity.EXTRA_GAME_ID, entry.id)
                .putExtra(GameActivity.EXTRA_GAME_NAME, entry.displayName),
        )
    }
}
