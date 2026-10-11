package com.pockethle.app

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import androidx.preference.EditTextPreference
import androidx.preference.ListPreference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.SwitchPreferenceCompat
import org.json.JSONObject

/** Per-game settings sheet. */
class GameSettingsActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)
        setSupportActionBar(findViewById<Toolbar>(R.id.toolbar))
        supportActionBar?.setDisplayHomeAsUpEnabled(true)
        title = intent.getStringExtra(EXTRA_GAME_NAME)
            ?: getString(R.string.game_settings_title)
        if (savedInstanceState == null) {
            supportFragmentManager.beginTransaction()
                .replace(
                    R.id.preferences_container,
                    GamePreferencesFragment().apply {
                        arguments = Bundle().apply {
                            putString(
                                EXTRA_GAME_ID,
                                intent.getStringExtra(EXTRA_GAME_ID),
                            )
                        }
                    },
                )
                .commit()
        }
    }

    override fun onSupportNavigateUp(): Boolean {
        finish()
        return true
    }

    class GamePreferencesFragment : PreferenceFragmentCompat() {
        private lateinit var rootDir: String
        private lateinit var gameId: String
        private var current: GameSettings = GameSettings.default()

        override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
            rootDir = LibraryPaths.root(requireContext())
            gameId = arguments?.getString(EXTRA_GAME_ID).orEmpty()
            current = readGameSettings() ?: GameSettings.default()
            setPreferencesFromResource(R.xml.preferences_game, rootKey)
            findPreference<ListPreference>("cpu_backend")?.apply {
                value = current.cpuBackend
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(cpuBackend = newValue.toString())
                    writeGameSettings()
                    true
                }
            }
            findPreference<EditTextPreference>("max_slices")?.apply {
                text = current.maxSlices.toString()
                setOnPreferenceChangeListener { _, newValue ->
                    val parsed = newValue.toString().toLongOrNull()
                    if (parsed != null && parsed > 0) {
                        current = current.copy(maxSlices = parsed)
                        writeGameSettings()
                        true
                    } else false
                }
            }
            findPreference<EditTextPreference>("instructions_per_slice")?.apply {
                text = current.instructionsPerSlice.toString()
                setOnPreferenceChangeListener { _, newValue ->
                    val parsed = newValue.toString().toLongOrNull()
                    if (parsed != null && parsed > 0) {
                        current = current.copy(instructionsPerSlice = parsed)
                        writeGameSettings()
                        true
                    } else false
                }
            }
            findPreference<ListPreference>("screen")?.apply {
                value = current.screen
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(screen = newValue.toString())
                    writeGameSettings()
                    true
                }
            }
            findPreference<ListPreference>("dpad_deadzone")?.apply {
                value = current.dpadDeadzone
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(dpadDeadzone = newValue.toString())
                    writeGameSettings()
                    true
                }
            }
            findPreference<ListPreference>("rotation")?.apply {
                value = current.rotation
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(rotation = newValue.toString())
                    writeGameSettings()
                    true
                }
            }
            findPreference<ListPreference>("gpu_renderer_mode")?.apply {
                // JSON is authoritative; do not maintain a second SharedPreferences value.
                isPersistent = false
                entries = arrayOf("Emulator default", "Software", "GPU (experimental)")
                entryValues = arrayOf("inherit", "software", "gpu")
                value = when (current.gpuRenderer) { null -> "inherit"; false -> "software"; true -> "gpu" }
                summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(gpuRenderer = when (newValue.toString()) { "gpu" -> true; "software" -> false; else -> null })
                    writeGameSettings()
                    true
                }
            }
            findPreference<SwitchPreferenceCompat>("halt_on_unimplemented")?.apply {
                isChecked = current.haltOnUnimplemented
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(haltOnUnimplemented = newValue as Boolean)
                    writeGameSettings()
                    true
                }
            }
        }

        private fun readGameSettings(): GameSettings? {
            if (gameId.isEmpty()) return null
            val raw = NativeBridge.readGameSettings(rootDir, gameId)
            return runCatching {
                val obj = JSONObject(raw)
                if (obj.has("ok") && !obj.optBoolean("ok", true)) null
                else GameSettings.fromJson(obj)
            }.getOrNull()
        }

        private fun writeGameSettings() {
            if (gameId.isEmpty()) return
            val result = runCatching { org.json.JSONObject(NativeBridge.writeGameSettings(rootDir, gameId, current.toJson().toString())) }.getOrNull()
            val saved = runCatching { org.json.JSONObject(NativeBridge.readGameSettings(rootDir, gameId)) }.getOrNull()
            val gpuSaved = saved != null && saved.has("gpu_renderer") &&
                (if (saved.isNull("gpu_renderer")) null else saved.optBoolean("gpu_renderer")) == current.gpuRenderer
            if (result?.optBoolean("ok", false) != true || !gpuSaved) {
                android.widget.Toast.makeText(context, "Could not save settings. Rebuild and install the updated native libraries and APK.", android.widget.Toast.LENGTH_LONG).show()
            }
        }
    }

    companion object {
        const val EXTRA_GAME_ID = "com.pockethle.app.EXTRA_GAME_ID_SETTINGS"
        const val EXTRA_GAME_NAME = "com.pockethle.app.EXTRA_GAME_NAME_SETTINGS"
    }
}
