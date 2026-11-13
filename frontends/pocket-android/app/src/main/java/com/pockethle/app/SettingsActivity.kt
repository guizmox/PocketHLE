package com.pockethle.app

import android.os.Bundle
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.widget.Toolbar
import androidx.lifecycle.lifecycleScope
import androidx.preference.CheckBoxPreference
import androidx.preference.ListPreference
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.SeekBarPreference
import androidx.preference.SwitchPreferenceCompat
import org.json.JSONObject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Global launcher settings (default backend, log verbosity). */
class SettingsActivity : HostKeyActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)
        setSupportActionBar(findViewById<Toolbar>(R.id.toolbar))
        supportActionBar?.setDisplayHomeAsUpEnabled(true)
        if (savedInstanceState == null) {
            supportFragmentManager.beginTransaction()
                .replace(R.id.preferences_container, GlobalPreferencesFragment())
                .commit()
        }
    }

    override fun onSupportNavigateUp(): Boolean {
        if (supportFragmentManager.backStackEntryCount > 0) supportFragmentManager.popBackStack() else finish()
        return true
    }

    class GlobalPreferencesFragment : ScrollingTitlePreferenceFragment() {
        private lateinit var rootDir: String
        private var current: LauncherConfig = LauncherConfig.default()
        override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
            rootDir = LibraryPaths.root(requireContext())
            current = readConfig() ?: LauncherConfig.default()
            preferenceManager.preferenceDataStore = null
            setPreferencesFromResource(R.xml.preferences_global, null)
            val giz = androidx.preference.PreferenceScreen(requireContext(), null).apply {
                key = "gizmondo_options"; title = "Gizmondo options"
            }
            preferenceScreen.addPreference(giz)
            val ppc = androidx.preference.PreferenceScreen(requireContext(), null).apply {
                key = "pocketpc_options"; title = "PocketPC options"
            }
            preferenceScreen.addPreference(ppc)
            fun identityField(screen: androidx.preference.PreferenceScreen, name: String, label: String) {
                screen.addPreference(androidx.preference.EditTextPreference(requireContext()).apply {
                    key = name; title = label; isPersistent = false
                    val preset = name.endsWith("preset_id")
                    val gizmondo = name.startsWith("gizmondo_")
                    val stored = current.toJson().optString(name)
                    text = if (preset) runCatching {
                        require(stored.length % 2 == 0)
                        val bytes = stored.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
                        if (gizmondo) {
                            require(bytes.size % 2 == 0)
                            val words = bytes.toString(Charsets.UTF_16LE)
                            require(!words.contains('\uFFFD'))
                            words.removeSuffix("\u0000").also { require(!it.contains('\u0000')) }
                        } else {
                            require(bytes.all { it.toInt() in 1..127 })
                            bytes.toString(Charsets.US_ASCII)
                        }
                    }.getOrElse { "" } else stored
                    summaryProvider = androidx.preference.EditTextPreference.SimpleSummaryProvider.getInstance()
                    dialogMessage = if (preset) "Device name. Encoded automatically; applies at the next launch." else "Hexadecimal console identifier. Applies at the next launch."
                    setOnPreferenceChangeListener { _, value ->
                        val input = value.toString()
                        val hex = if (preset) {
                            if (input.isEmpty() || input.contains('\u0000') || (!gizmondo && input.any { it.code > 127 })) {
                                android.widget.Toast.makeText(requireContext(), "Enter a device name" + if (gizmondo) "" else " using ASCII characters", android.widget.Toast.LENGTH_LONG).show()
                                return@setOnPreferenceChangeListener false
                            }
                            val bytes = if (gizmondo) (input + "\u0000").toByteArray(Charsets.UTF_16LE) else input.toByteArray(Charsets.US_ASCII)
                            bytes.joinToString("") { "%02X".format(it.toInt() and 255) }
                        } else input.trim()
                        if (hex.isEmpty() || hex.length % 2 != 0 || hex.length > 8192 || !hex.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
                            android.widget.Toast.makeText(requireContext(), "Enter pairs of hexadecimal digits (1–4096 bytes)", android.widget.Toast.LENGTH_LONG).show()
                            false
                        } else {
                            current = current.copy(originalJson = current.toJson().put(name, hex).toString())
                            writeConfig(); true
                        }
                    }
                })
            }
            val touch = androidx.preference.PreferenceCategory(requireContext()).apply { title = "Touch controls" }
            ppc.addPreference(touch)
            touch.addPreference(ListPreference(requireContext()).apply {
                key = "android_pocketpc_touch_layout"; title = "Touch layout"; dialogTitle = "Touch layout"; isPersistent = false
                entries = arrayOf("Classic", "Controller"); entryValues = arrayOf("classic", "controller")
                value = current.pocketpcTouchLayout
                summaryProvider = Preference.SummaryProvider<ListPreference> { preference ->
                    "${preference.entry ?: "Classic"}\nController aligns P1–P4 and the D-pad/action diamond, with a separate stylus stick. Works with Normal and Big sizes in Display and controls. Applies next launch."
                }
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(pocketpcTouchLayout = value.toString()); writeConfig(); true
                }
            })
            val pocketpc=androidx.preference.PreferenceCategory(requireContext()).apply { title="Device profile" }
            ppc.addPreference(pocketpc)
            pocketpc.addPreference(ListPreference(requireContext()).apply {
                key="pocketpc_processor"; title="Processor profile"; isPersistent=false
                entries=arrayOf("Intel PXA270 (Dell Axim X51v)","Intel PXA255","StrongARM SA-1110","Generic ARM")
                entryValues=arrayOf("pxa270","pxa255","sa1110","arm")
                value=current.toJson().optString("pocketpc_processor","pxa270")
                // AlertDialog displays its message instead of the single-choice
                // list when both are supplied. Keep help on the settings row.
                dialogTitle="Processor profile"
                summaryProvider=Preference.SummaryProvider<ListPreference> { preference ->
                    "${preference.entry ?: "Intel PXA270 (Dell Axim X51v)"}\nIdentity reported to ARM PocketPC games. Applies next launch; does not change CPU speed or backend. MIPS games retain MIPS identity."
                }
                setOnPreferenceChangeListener { _,v ->
                    current=current.copy(originalJson=current.toJson().put("pocketpc_processor",v.toString()).toString())
                    writeConfig();true
                }
            })
            pocketpc.addPreference(ListPreference(requireContext()).apply {
                key = "pocketpc_tilt_input"
                title = "Tilt input (accelerometer)"
                isPersistent = false
                entries = TiltInput.values().map { it.label }.toTypedArray()
                entryValues = TiltInput.values().map { it.id }.toTypedArray()
                value = current.pocketpcTiltInput.id
                summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, value ->
                    val input = TiltInput.fromId(value.toString()) ?: return@setOnPreferenceChangeListener false
                    current = current.copy(pocketpcTiltInput = input)
                    writeConfig()
                    true
                }
            })
            identityField(ppc, "device_preset_id", "Device name (Preset ID — WinCE)")
            identityField(ppc, "device_platform_id", "Console identifier (Platform ID — hexadecimal, WinCE)")
            identityField(giz, "gizmondo_preset_id", "Device name (Preset ID — Gizmondo)")
            identityField(giz, "gizmondo_platform_id", "Console identifier (Platform ID — hexadecimal, Gizmondo)")
            fun toggle(keyName: String, label: String, checked: Boolean, update: (Boolean) -> Unit) {
                giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                    key = keyName; title = label; isPersistent = false; isChecked = checked
                    setOnPreferenceChangeListener { _, value -> update(value as Boolean); writeConfig(); true }
                })
            }
            toggle("gprs_enabled", "GPRS/data", current.gprsEnabled) { current = current.copy(gprsEnabled = it) }
            val display = findPreference<androidx.preference.PreferenceScreen>("display_options")!!
            display.addPreference(ListPreference(requireContext()).apply {
                key = "upscale_filter"; title = "Filter"; isPersistent = false
                entries = arrayOf("No filter", "Reconstruction", "SMAA", "SMAA Soft", "xBRZ", "Bicubic", "Lanczos", "Bilinear", "Nearest")
                entryValues = arrayOf("none", "reconstruction", "smaa", "smaa_soft", "xbrz", "bicubic", "lanczos", "bilinear", "nearest")
                value = current.upscaleFilter; summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, v -> current = current.copy(upscaleFilter = v.toString()); writeConfig(); true }
            })
            giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "bluetooth_enabled"
                title = "Bluetooth hardware (Classic / RFCOMM)"
                summary = "Use the device radio for Gizmondo Bluetooth games"
                isPersistent = false
                isChecked = current.bluetoothEnabled
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(bluetoothEnabled = value as Boolean)
                    writeConfig()
                    true
                }
            })
            giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "camera_enabled"
                title = "Camera hardware (CAM1)"
                summary = "Allow games to use the rear camera (next launch)"
                isPersistent = false
                isChecked = current.cameraEnabled
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(cameraEnabled = value as Boolean)
                    writeConfig()
                    true
                }
            })
            giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "gps_enabled"
                title = "GPS / device location (GPS1)"
                summary = "Allow games to read real device location. Colors Fake GPS is selected at launch."
                isPersistent = false
                isChecked = current.gpsEnabled
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(gpsEnabled = value as Boolean)
                    writeConfig(); true
                }
            })
            val input = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="input_options";title="Controls" }
            preferenceScreen.addPreference(input)
            fun directionalSize(gizmondo: Boolean) {
                val name = if (gizmondo) "Gizmondo arrows and action buttons size" else "PocketPC arrows and stick size"
                display.addPreference(ListPreference(requireContext()).apply {
                    key = if (gizmondo) "android_gizmondo_controls_size" else "android_pocketpc_controls_size"
                    title = name; dialogTitle = name; isPersistent = false
                    entries = arrayOf("Normal", "Big"); entryValues = arrayOf("normal", "big")
                    value = if (gizmondo) current.gizmondoControlsSize else current.pocketpcControlsSize
                    summaryProvider = Preference.SummaryProvider<ListPreference> { pref ->
                        "${pref.entry ?: "Normal"}\nGlobal setting. Big enlarges ${if (gizmondo) "arrows and action buttons" else "directional controls"} by about one third, limited by available space. The game keeps its aspect ratio; its display scale may decrease. Applies next launch."
                    }
                    setOnPreferenceChangeListener { _, value ->
                        current = if (gizmondo) current.copy(gizmondoControlsSize = value.toString())
                            else current.copy(pocketpcControlsSize = value.toString())
                        writeConfig(); true
                    }
                })
            }
            directionalSize(false); directionalSize(true)
            giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "android_gizmondo_maximize_screen"
                title = "Maximize screen size by allowing controls to overflow it"
                summary = "Use the whole available screen for the game, with translucent controls over it. Keeps the game aspect ratio and selected scale. Applies next launch."
                isPersistent = false; isChecked = current.gizmondoMaximizeScreen
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(gizmondoMaximizeScreen = value as Boolean)
                    writeConfig(); true
                }
            })
            for ((controller, label) in listOf(false to "Keyboard bindings", true to "External controller bindings")) {
                input.addPreference(Preference(requireContext()).apply {
                    key = if (controller) "controller_bindings" else "keyboard_bindings"
                    title = label
                    summary = if (controller) "Inspect and edit external controller assignments." else "Inspect and edit keyboard assignments."
                    setOnPreferenceClickListener {
                        startActivity(android.content.Intent(requireContext(), InputSettingsActivity::class.java).putExtra("controller", controller))
                        true
                    }
                })
            }
            findPreference<androidx.preference.PreferenceCategory>("diagnostic_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key="log_unimplemented_apis";title="Unimplemented API report";isPersistent=false;isChecked=current.logUnimplementedApis
                summary="Writes pockethle-unimplemented.log in the library folder."
                setOnPreferenceChangeListener { _,v -> current=current.copy(logUnimplementedApis=v as Boolean);writeConfig();true }
            })
            findPreference<androidx.preference.PreferenceCategory>("graphics_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key="gpu_renderer"; title="GPU rendering"; isPersistent=false; isChecked=current.gpuRenderer
                summary="Host OpenGL for OpenGL ES 1.x. Takes effect the next time a game launches."
                setOnPreferenceChangeListener { _, v ->
                    current = current.copy(gpuRenderer = v as Boolean)
                    writeConfig()
                    true
                }
            })
            findPreference<androidx.preference.PreferenceCategory>("graphics_options")!!.addPreference(ListPreference(requireContext()).apply {
                key = "gles_internal_resolution"
                title = "OpenGL ES internal resolution (host OpenGL)"
                isPersistent = false
                entries = arrayOf("Native", "2×", "3×", "4×")
                entryValues = arrayOf("1", "2", "3", "4")
                value = current.glesResolutionScale.toString()
                summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, v ->
                    current = current.withGlesResolution(v.toString().toInt(), current.glesResolutionAllSizes)
                    findPreference<CheckBoxPreference>("gles_resolution_all_sizes")?.apply {
                        isChecked = current.glesResolutionAllSizes
                        isEnabled = current.glesResolutionScale > 1
                    }
                    writeConfig()
                    true
                }
            })
            findPreference<androidx.preference.PreferenceCategory>("graphics_options")!!.addPreference(CheckBoxPreference(requireContext()).apply {
                key = "gles_resolution_all_sizes"
                title = "Apply to all resolutions, including sizes other than 320×240"
                summary = "When unchecked, only 320×240 and 240×320 are scaled. Applies the next time a game launches."
                isPersistent = false
                isChecked = current.glesResolutionAllSizes
                isEnabled = current.glesResolutionScale > 1
                setOnPreferenceChangeListener { _, v ->
                    current = current.withGlesResolution(current.glesResolutionScale, v as Boolean)
                    writeConfig()
                    true
                }
            })
            findPreference<androidx.preference.PreferenceCategory>("graphics_options")!!.addPreference(ListPreference(requireContext()).apply {
                key="frame_rate_limit"; title="Frame rate limit"; isPersistent=false
                entries=arrayOf("10 fps", "15 fps", "20 fps", "30 fps", "45 fps", "60 fps", "120 fps", "Unlimited")
                entryValues=arrayOf("10", "15", "20", "30", "45", "60", "120", "unlimited")
                value=current.frameRateLimit
                summaryProvider=Preference.SummaryProvider<ListPreference> { pref -> "${pref.entry ?: "60 fps"}. Default maximum game FPS (or Unlimited); each game can inherit or override it. Applies next launch." }
                setOnPreferenceChangeListener { _, v -> current=current.copy(frameRateLimit=v.toString());writeConfig();true }
            })


            findPreference<androidx.preference.PreferenceCategory>("misc_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "executable_heap"
                title = "Executable heap"
                isPersistent = false
                isChecked = current.executableHeap
                setSummary(R.string.executable_heap_help)
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(executableHeap = value as Boolean)
                    writeConfig()
                    true
                }
            })
            findPreference<androidx.preference.PreferenceCategory>("misc_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "show_fps"; title = "Show FPS"; isPersistent = false; isChecked = current.showFps
                summary = "Shows a small counter of rendered frames on the game screen. Applies on the next launch."
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(showFps = value as Boolean)
                    writeConfig()
                    true
                }
            })
            findPreference<androidx.preference.PreferenceCategory>("misc_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "show_ips"; title = "Show IPS"; isPersistent = false; isChecked = current.showIps
                summary = "Shows changed images per second at the top right. Applies on next launch. Uses CPU pixel readback for OpenGL ES."
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(showIps = value as Boolean)
                    writeConfig()
                    true
                }
            })
            findPreference<SwitchPreferenceCompat>("android_graphics_diagnostics")?.apply {
                isChecked = current.graphicsDiagnostics
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(graphicsDiagnostics = value as Boolean)
                    writeConfig(); true
                }
            }
            findPreference<SeekBarPreference>("verbosity")?.apply {
                value = current.verbosity
                setOnPreferenceChangeListener { _, newValue ->
                    current = current.copy(verbosity = (newValue as Int))
                    writeConfig()
                    true
                }
            }
            // Stored as a 0..1 float in `config.json` (the desktop
            // launcher reads the same field), shown here as a percentage
            // because a SeekBarPreference only deals in ints.
            findPreference<SeekBarPreference>("controls_opacity")?.apply {
                value = (current.controlsOpacity * 100f).toInt().coerceIn(10, 100)
                setOnPreferenceChangeListener { _, newValue ->
                    val percent = (newValue as Int).coerceIn(10, 100)
                    current = current.copy(controlsOpacity = percent / 100f)
                    writeConfig()
                    true
                }
            }
            findPreference<Preference>("library_root")?.summary = rootDir
            findPreference<Preference>("cleanup_cab")?.setOnPreferenceClickListener { preference ->
                preference.isEnabled = false
                lifecycleScope.launch {
                    val result = withContext(Dispatchers.IO) {
                        runCatching { JSONObject(NativeBridge.cleanupCabInstallFiles(rootDir)) }
                    }
                    preference.isEnabled = true
                    val response = result.getOrNull()
                    if (isAdded) {
                        val ok = response?.optBoolean("ok") == true
                        AlertDialog.Builder(requireContext())
                            .setTitle(if (ok) "CAB cleanup complete" else "CAB cleanup failed")
                            .setMessage(
                                response?.optString(if (ok) "message" else "error")
                                    ?: result.exceptionOrNull()?.localizedMessage
                                    ?: "Could not check CAB install files."
                            )
                            .setPositiveButton(android.R.string.ok, null)
                            .show()
                    }
                }
                true
            }
            if (rootKey != null) preferenceScreen = findPreference<androidx.preference.PreferenceScreen>(rootKey)!!
        }

        override fun onResume() {
            super.onResume()
            // Reload independently saved visual bindings before any later preference write.
            current=readConfig() ?: current
        }

        override fun onPreferenceTreeClick(preference: Preference): Boolean {
            if (preference is androidx.preference.PreferenceScreen) {
                parentFragmentManager.beginTransaction()
                    .replace(R.id.preferences_container, GlobalPreferencesFragment().apply { arguments = Bundle().apply { putString(PreferenceFragmentCompat.ARG_PREFERENCE_ROOT, preference.key) } })
                    .addToBackStack(null).commit()
                return true
            }
            return super.onPreferenceTreeClick(preference)
        }

        private fun readConfig(): LauncherConfig? {
            val raw = NativeBridge.readConfig(rootDir)
            return runCatching {
                val obj = JSONObject(raw)
                if (obj.has("ok") && !obj.optBoolean("ok", true)) null
                else LauncherConfig.fromJson(obj)
            }.getOrNull()
        }

        private fun writeConfig() {
            val result=runCatching { JSONObject(NativeBridge.writeConfig(rootDir,current.toJson().toString())) }.getOrNull()
            val saved = runCatching { JSONObject(NativeBridge.readConfig(rootDir)) }.getOrNull()
            if (result?.optBoolean("ok", false) == true && saved?.optString("android_pocketpc_touch_layout", "") != current.pocketpcTouchLayout) {
                android.widget.Toast.makeText(context, "Touch layout could not be saved. Please update PocketHLE and try again.", android.widget.Toast.LENGTH_LONG).show()
            }
            if (result?.optBoolean("ok", false) == true &&
                (saved == null || !saved.has("android_gizmondo_maximize_screen") ||
                    saved.optBoolean("android_gizmondo_maximize_screen") != current.gizmondoMaximizeScreen)) {
                android.widget.Toast.makeText(context, "Screen size setting was not saved. Rebuild the native libraries and APK together.", android.widget.Toast.LENGTH_LONG).show()
            }
            val gpuSaved = saved != null && saved.optString("frame_rate_limit", "") == current.frameRateLimit && saved.has("gpu_renderer") && saved.optBoolean("gpu_renderer") == current.gpuRenderer && saved.has("gles_internal_resolution") && saved.optInt("gles_internal_resolution") == current.glesInternalResolution
            if (result?.optBoolean("ok", false) == true && saved?.optString("android_gizmondo_controls_size", "") != current.gizmondoControlsSize) {
                android.widget.Toast.makeText(context, "Control size could not be saved. Please update PocketHLE and try again.", android.widget.Toast.LENGTH_LONG).show()
            }
            if (result?.optBoolean("ok", false) == true && saved?.optString("android_pocketpc_controls_size", "") != current.pocketpcControlsSize) {
                android.widget.Toast.makeText(context, "Control size was not saved. Rebuild and install the updated native libraries and APK.", android.widget.Toast.LENGTH_LONG).show()
            }
            if (result?.optBoolean("ok", false) == true &&
                (saved == null || !saved.has("executable_heap") ||
                    saved.optBoolean("executable_heap") != current.executableHeap)) {
                android.widget.Toast.makeText(context, "Executable heap setting was not saved. Rebuild the native libraries and APK together.", android.widget.Toast.LENGTH_LONG).show()
            }
            if (!gpuSaved) android.widget.Toast.makeText(context, "GPU setting was not saved. Rebuild and install the updated native libraries and APK.", android.widget.Toast.LENGTH_LONG).show()
            if(result?.optBoolean("ok",false)!=true) android.widget.Toast.makeText(context,result?.optString("error") ?: "Could not save settings",android.widget.Toast.LENGTH_LONG).show()
        }
    }
}
