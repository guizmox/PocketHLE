package com.pockethle.app

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import androidx.preference.ListPreference
import androidx.preference.PreferenceFragmentCompat
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
            findPreference<ListPreference>("screen")?.isVisible =
                !NativeBridge.isGizmondoGame(rootDir, gameId)
            findPreference<androidx.preference.PreferenceCategory>("device_identity_category")?.isVisible =
                !NativeBridge.isGizmondoGame(rootDir, gameId)
            if (!NativeBridge.isGizmondoGame(rootDir, gameId)) {
                val controlsCategory = findPreference<androidx.preference.PreferenceCategory>("controls_category")!!
                val identityCategory = findPreference<androidx.preference.PreferenceCategory>("device_identity_category")!!
                val editor=androidx.preference.Preference(requireContext()).apply {
                    key="ppc_stylus_editor";isPersistent=false
                    setOnPreferenceClickListener {
                        val config=LauncherConfig.fromJson(JSONObject(NativeBridge.readConfig(rootDir)))
                        StylusControlsDialog(requireContext(),current,config){grid->
                            current=current.copy(pocketpcStylusGridJson=grid.json().toString());writeGameSettings()
                        }.show()
                        true
                    }
                }
                fun updateEditor() {
                    editor.isVisible=current.controlsMode()!="normal"
                    editor.title=if(current.controlsMode()=="zones") "Test controls…" else "Configure grid…"
                    editor.summary=if(current.controlsMode()=="zones") "Test arrows / D-pad and progressive left-stick movement without starting a game." else "Choose 4 × 3, 5 × 4 or 6 × 5. Select a zone and press an input. Reserved PocketHLE shortcuts cannot be assigned."
                }
                controlsCategory.addPreference(ListPreference(requireContext()).apply {
                    key="pocketpc_controls";title="Control mode";isPersistent=false
                    entries=arrayOf("Normal","Touch steering","Touch zone mapping")
                    entryValues=arrayOf("normal","zones","grid");value=current.controlsMode()
                    summaryProvider=androidx.preference.Preference.SummaryProvider<ListPreference> { current.controlsDescription() }
                    setOnPreferenceChangeListener {_,v->
                        current=current.copy(pocketpcControls=v.toString(),pocketpcTouchSteering=v.toString()=="zones")
                        writeGameSettings();updateEditor();true
                    }
                })
                controlsCategory.addPreference(editor);updateEditor()
                fun identityField(keyName: String, label: String, initial: String, update: (String) -> Unit) {
                    identityCategory.addPreference(androidx.preference.EditTextPreference(requireContext()).apply {
                        key = keyName; title = label; isPersistent = false; text = initial
                        summaryProvider = androidx.preference.EditTextPreference.SimpleSummaryProvider.getInstance()
                        dialogMessage = "Empty = device default. Applies at the next launch." + if ((keyName.endsWith("device_id") || keyName.endsWith("preset_id"))) " Enter hexadecimal byte pairs." else ""
                        if (keyName == "pocketpc_random_seed") {
                            dialogMessage = "Compatibility setting, not hardware identity. Use to reproduce a legacy registration request. Decimal 0–4294967295; empty = normal randomness. Overrides every srand seed for this game."
                            setOnBindEditTextListener { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER }
                        }
                        setOnPreferenceChangeListener { _, value ->
                            val input = value.toString()
                            val valid = !input.contains('\u0000') && when (keyName) {
                                "pocketpc_random_seed" -> input.isEmpty() || (input.all { it in '0'..'9' } && input.toLongOrNull()?.let { it in 0L..4294967295L } == true)
                                "pocketpc_oem_info", "pocketpc_platform_type", "pocketpc_owner_name" -> input.length <= 255
                                "pocketpc_device_name" -> input.length <= 4096 && input.all { it.code <= 127 }
                                else -> input.length <= 8192 && input.length % 2 == 0 && input.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
                            }
                            if (!valid) {android.widget.Toast.makeText(requireContext(), "Invalid identity value", android.widget.Toast.LENGTH_LONG).show();false}
                            else {update(input);writeGameSettings();true}
                        }
                    })
                }
                identityField("pocketpc_oem_info", "OEM model (readable text)", current.pocketpcOemInfo) {current=current.copy(pocketpcOemInfo=it)}
                identityField("pocketpc_platform_type", "Platform type (readable text)", current.pocketpcPlatformType) {current=current.copy(pocketpcPlatformType=it)}
                findPreference<androidx.preference.PreferenceCategory>("graphics_category")!!.addPreference(ListPreference(requireContext()).apply {
                    key="pocketpc_gapi_columns";title="GAPI display compatibility";isPersistent=false
                    entries=arrayOf("Linear scanlines (default)","Columns (early portrait LCD)")
                    entryValues=arrayOf("false","true");value=current.pocketpcGapiColumns.toString()
                    summaryProvider=androidx.preference.Preference.SummaryProvider<ListPreference> { "${it.entry}\nUse Columns if a PocketPC game's image or text wraps around the screen or appears shifted. Restart the game after changing this compatibility setting." }
                    // AlertDialog's message hides its single-choice list.
                    // The settings-row summary already contains the help text.
                    dialogTitle="GAPI display compatibility"
                    setOnPreferenceChangeListener { _, value ->
                        current=current.copy(pocketpcGapiColumns=value.toString()=="true");writeGameSettings();true
                    }
                })
                identityField("pocketpc_random_seed", "Random seed override (compatibility)", current.pocketpcRandomSeed) {current=current.copy(pocketpcRandomSeed=it)}
                identityField("pocketpc_owner_name", "Owner name (PocketPC)", current.pocketpcOwnerName) {current=current.copy(pocketpcOwnerName=it)}
                identityField("pocketpc_device_name", "Device name (Preset ID — ASCII)", current.pocketpcDeviceName) {current=current.copy(pocketpcDeviceName=it)}
                identityField("pocketpc_preset_id", "Preset ID override (hexadecimal)", current.pocketpcPresetId) {current=current.copy(pocketpcPresetId=it)}
                identityField("pocketpc_device_id", "PocketPC ID (Platform ID — hexadecimal)", current.pocketpcDeviceId) {current=current.copy(pocketpcDeviceId=it)}
            }
            findPreference<ListPreference>("screen")?.apply {
                value = current.screen
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(screen = newValue.toString())
                    writeGameSettings()
                    true
                }
            }
            findPreference<ListPreference>("frame_rate_limit")?.apply {
                isPersistent = false
                entries = arrayOf("Unlimited", "10 fps", "15 fps", "20 fps", "30 fps", "45 fps", "60 fps")
                entryValues = arrayOf("unlimited", "10", "15", "20", "30", "45", "60")
                value = current.frameRateLimit
                summaryProvider = androidx.preference.Preference.SummaryProvider<ListPreference> { pref ->
                    "${pref.entry ?: "Unlimited"}. Maximum game FPS; may help frame-dependent animations. Unlimited adds no cap; display sync still applies. GAPI / DirectDraw / OpenGL ES; GDI-only drawing is not capped. Restart the game to apply."
                }
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(frameRateLimit = newValue.toString())
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
            val globalGpu = runCatching { JSONObject(NativeBridge.readConfig(rootDir)).optBoolean("gpu_renderer", true) }.getOrDefault(true)
            findPreference<ListPreference>("anisotropic_filtering")?.apply {
                isPersistent=false
                entries=arrayOf("Emulator default", "Off", "2×", "4×", "8×")
                entryValues=arrayOf("inherit", "0", "2", "4", "8")
                value=current.anisotropicFiltering?.toString() ?: "inherit"
                isEnabled=current.gpuRenderer ?: globalGpu
                summaryProvider=ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, v ->
                    current=current.copy(anisotropicFiltering=v.toString().toIntOrNull())
                    writeGameSettings(); true
                }
            }
            findPreference<ListPreference>("gpu_renderer_mode")?.apply {
                // JSON is authoritative; do not maintain a second SharedPreferences value.
                isPersistent = false
                entries = arrayOf("Emulator default", "Software", "GPU")
                entryValues = arrayOf("inherit", "software", "gpu")
                value = when (current.gpuRenderer) { null -> "inherit"; false -> "software"; true -> "gpu" }
                summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(gpuRenderer = when (newValue.toString()) { "gpu" -> true; "software" -> false; else -> null })
                    findPreference<ListPreference>("anisotropic_filtering")?.isEnabled=current.gpuRenderer ?: globalGpu
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
                (if (saved.isNull("gpu_renderer")) null else saved.optBoolean("gpu_renderer")) == current.gpuRenderer && saved.has("anisotropic_filtering") &&
                (if (saved.isNull("anisotropic_filtering")) null else saved.optInt("anisotropic_filtering")) == current.anisotropicFiltering
            val frameLimitSaved = saved?.optString("frame_rate_limit", "") == current.frameRateLimit
            if (result?.optBoolean("ok", false) != true || !gpuSaved || !frameLimitSaved) {
                android.widget.Toast.makeText(context, "Could not save settings. Rebuild and install the updated native libraries and APK.", android.widget.Toast.LENGTH_LONG).show()
            }
        }
    }

    companion object {
        const val EXTRA_GAME_ID = "com.pockethle.app.EXTRA_GAME_ID_SETTINGS"
        const val EXTRA_GAME_NAME = "com.pockethle.app.EXTRA_GAME_NAME_SETTINGS"
    }
}
