package com.pockethle.app

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import androidx.preference.ListPreference
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.SeekBarPreference
import androidx.preference.SwitchPreferenceCompat
import org.json.JSONObject

/** Global launcher settings (default backend, log verbosity). */
class SettingsActivity : AppCompatActivity() {
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

    class GlobalPreferencesFragment : PreferenceFragmentCompat() {
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
            val emulator = findPreference<androidx.preference.PreferenceScreen>("emulator_options")!!
            val pocketpc=androidx.preference.PreferenceCategory(requireContext()).apply { title="PocketPC" }
            emulator.addPreference(pocketpc)
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
            identityField(emulator, "device_preset_id", "Device name (Preset ID — WinCE)")
            identityField(emulator, "device_platform_id", "Console identifier (Platform ID — hexadecimal, WinCE)")
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
                entries = arrayOf("Reconstruction", "SMAA", "SMAA Soft", "xBRZ", "Bicubic", "Lanczos", "Bilinear", "Nearest")
                entryValues = arrayOf("reconstruction", "smaa", "smaa_soft", "xbrz", "bicubic", "lanczos", "bilinear", "nearest")
                value = current.upscaleFilter; summaryProvider = ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, v -> current = current.copy(upscaleFilter = v.toString()); writeConfig(); true }
            })
            display.addPreference(ListPreference(requireContext()).apply {
                key = "android_display_scale"; title = "Display scale"; isPersistent = false
                entries = arrayOf("Auto (largest integer scale that fits)", "QVGA ×1", "QVGA ×2", "QVGA ×3", "QVGA ×4")
                entryValues = arrayOf("0", "1", "2", "3", "4"); value = current.displayScale.toString()
                summary = "Pocket PC: multiples of the QVGA grid, including VGA games. Gizmondo: native multiples. Limited to the available space."
                setOnPreferenceChangeListener { _, v -> current = current.copy(displayScale = v.toString().toInt()); writeConfig(); true }
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
            val input = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="input_options";title="Keyboard and controllers" }
            preferenceScreen.addPreference(input)
            val keyboard = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="keyboard_options";title="Physical keyboard" }
            val gamepad = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="gamepad_options";title="Physical controller" }
            input.addPreference(keyboard);input.addPreference(gamepad)
            val controlGroups = listOf(
                "Shared controls — Pocket PC and Gizmondo",
                "Emulator shortcuts",
                "Gizmondo only — console system buttons",
                "Pocket PC only — hardware buttons"
            )
            fun categories(screen: androidx.preference.PreferenceScreen) = controlGroups.associateWith { group ->
                androidx.preference.PreferenceCategory(requireContext()).apply {
                    title = group
                    screen.addPreference(this)
                }
            }
            val keyboardGroups = categories(keyboard)
            val gamepadGroups = categories(gamepad)
            val stylusScreen=androidx.preference.PreferenceScreen(requireContext(),null).apply {
                title="Pocket PC only — stylus stick";key="stylus_options"
            }
            input.addPreference(stylusScreen)
            fun stylusJson()=current.toJson().optJSONObject("stylus_stick") ?: org.json.JSONObject().apply {
                put("enabled",true);put("speed",240)
                put("keyboard",org.json.JSONArray().apply { listOf("O","L","K","M").forEach { put(org.json.JSONArray().put(it)) } })
                put("controller",org.json.JSONArray().apply { StylusBindings.directions.forEach { put(org.json.JSONArray().put("RightStick$it")) } })
            }
            fun saveStylus(value:org.json.JSONObject) {
                current=current.copy(originalJson=current.toJson().put("stylus_stick",value).toString());writeConfig()
            }
            stylusScreen.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                title="Enable stylus stick";summary="Emulates stylus drags; lifts and recentres at screen edges. Pocket PC only."
                isPersistent=false;isChecked=stylusJson().optBoolean("enabled",true)
                setOnPreferenceChangeListener { _,v -> saveStylus(stylusJson().put("enabled",v as Boolean));true }
            })
            stylusScreen.addPreference(androidx.preference.SeekBarPreference(requireContext()).apply {
                title="Stylus speed (pixels/s)";min=30;max=1200;showSeekBarValue=true;isPersistent=false
                value=stylusJson().optInt("speed",240)
                setOnPreferenceChangeListener { _,v -> saveStylus(stylusJson().put("speed",v as Int));true }
            })
            StylusBindings.directions.forEachIndexed { i,direction ->
                stylusScreen.addPreference(Preference(requireContext()).apply {
                    title="Stylus $direction — keyboard";isPersistent=false
                    summary=stylusJson().getJSONArray("keyboard").getJSONArray(i).join(", ")
                    setOnPreferenceClickListener {
                        val dialog=androidx.appcompat.app.AlertDialog.Builder(requireContext()).setTitle("Press a key")
                            .setNegativeButton("Cancel",null).setNeutralButton("Clear") { _,_ ->
                                val j=stylusJson();j.getJSONArray("keyboard").put(i,org.json.JSONArray());saveStylus(j);summary="Unbound"
                            }.create()
                        dialog.setOnKeyListener { _,code,event ->
                            val name=InputBindings.keyName(event)
                            if(code==android.view.KeyEvent.KEYCODE_BACK) false
                            else if(event.action==android.view.KeyEvent.ACTION_DOWN && name!=null && name!="F10") {
                                val j=stylusJson();val a=j.getJSONArray("keyboard")
                                for(k in 0..3) {
                                    val old=a.getJSONArray(k);val next=org.json.JSONArray()
                                    for(n in 0 until old.length()) if(!InputBindings.canonical(old.getString(n)).equals(InputBindings.canonical(name),true)) next.put(old.getString(n))
                                    a.put(k,next)
                                }
                                a.put(i,org.json.JSONArray().put(name));saveStylus(j);summary=name;dialog.dismiss();true
                            } else false
                        };dialog.show();true
                    }
                })
                stylusScreen.addPreference(ListPreference(requireContext()).apply {
                    title="Stylus $direction — controller";isPersistent=false
                    val names=listOf("")+InputBindings.controls
                    entries=names.map { if(it.isEmpty()) "Unbound" else it }.toTypedArray();entryValues=names.toTypedArray()
                    value=stylusJson().getJSONArray("controller").getJSONArray(i).optString(0,"")
                    summaryProvider=ListPreference.SimpleSummaryProvider.getInstance()
                    setOnPreferenceChangeListener { _,v ->
                        val j=stylusJson();val a=j.getJSONArray("controller");val name=v.toString()
                        for(k in 0..3) { val old=a.getJSONArray(k);val next=org.json.JSONArray();for(n in 0 until old.length()) if(old.getString(n)!=name) next.put(old.getString(n));a.put(k,next) }
                        a.put(i,org.json.JSONArray().apply { if(name.isNotEmpty()) put(name) });saveStylus(j);true
                    }
                })
            }
            InputBindings.buttons.forEach { (button,info) ->
                val group = when {
                    button.startsWith("pocket_pc_app") -> controlGroups[3]
                    button.startsWith("giz_piano") -> controlGroups[2]
                    button == "turbo" -> controlGroups[1]
                    else -> controlGroups[0]
                }
                keyboardGroups.getValue(group).addPreference(Preference(requireContext()).apply {
                    key="keyboard_$button";title=info.first;isPersistent=false
                    val snapshot=InputBindings(current)
                    val existing=(0 until snapshot.keyboard.length()).map { snapshot.keyboard.getJSONObject(it) }.find { it.optString("button")==button }?.optJSONArray("keys")
                    summary=existing?.let { (0 until it.length()).joinToString(", ") { i -> it.getString(i) } }?.ifEmpty { "Unassigned" } ?: "Unassigned"
                    setOnPreferenceClickListener {
                        val dialog=androidx.appcompat.app.AlertDialog.Builder(requireContext()).setTitle(info.first).setMessage("Press a keyboard key (F10 is reserved for screenshots).")
                            .setNegativeButton("Cancel",null).setNeutralButton("Clear") { _,_-> saveKeyboardBinding(button,null);summary="Unassigned" }.create()
                        dialog.setOnKeyListener { _,code,event ->
                            val name=InputBindings.keyName(event)
                            if(code==android.view.KeyEvent.KEYCODE_BACK) false
                            else if(event.action==android.view.KeyEvent.ACTION_DOWN && name!=null && name!="F10") {
                                saveKeyboardBinding(button,name);summary=name;dialog.dismiss();true
                            } else false
                        }
                        dialog.show();true
                    }
                })
                gamepadGroups.getValue(group).addPreference(ListPreference(requireContext()).apply {
                    key="gamepad_$button";title=info.first;isPersistent=false
                    entries=(listOf("Unassigned")+InputBindings.controls).toTypedArray();entryValues=(listOf("")+InputBindings.controls).toTypedArray()
                    val map=InputBindings(current).controller
                    value=map.keys().asSequence().firstOrNull { map.optString(it)==button } ?: ""
                    summaryProvider=ListPreference.SimpleSummaryProvider.getInstance()
                    setOnPreferenceChangeListener { _,v ->
                        val next=InputBindings(current).controller
                        next.keys().asSequence().toList().filter { next.optString(it)==button }.forEach { next.remove(it) }
                        if(v.toString().isNotEmpty()) next.put(v.toString(),button)
                        current=current.copy(originalJson=current.toJson().put("gamepad_bindings",next).toString());writeConfig();true
                    }
                })
            }
            findPreference<androidx.preference.PreferenceScreen>("emulator_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key="log_unimplemented_apis";title="Unimplemented API report";isPersistent=false;isChecked=current.logUnimplementedApis
                summary="Writes pockethle-unimplemented.log in the library folder."
                setOnPreferenceChangeListener { _,v -> current=current.copy(logUnimplementedApis=v as Boolean);writeConfig();true }
            })
            findPreference<androidx.preference.PreferenceScreen>("emulator_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key="gpu_renderer"; title="GPU rendering"; isPersistent=false; isChecked=current.gpuRenderer
                summary="Host OpenGL; takes effect on the next launch. Can be overridden per game."
                setOnPreferenceChangeListener { _,v -> current=current.copy(gpuRenderer=v as Boolean);findPreference<ListPreference>("anisotropic_filtering")?.isEnabled=current.gpuRenderer;writeConfig();true }
            })
            findPreference<androidx.preference.PreferenceScreen>("emulator_options")!!.addPreference(ListPreference(requireContext()).apply {
                key="anisotropic_filtering"; title="Anisotropic filtering (GPU only)"; isPersistent=false
                entries=arrayOf("Off", "2×", "4×", "8×"); entryValues=arrayOf("0", "2", "4", "8")
                value=current.anisotropicFiltering.toString(); isEnabled=current.gpuRenderer
                summaryProvider=ListPreference.SimpleSummaryProvider.getInstance()
                setOnPreferenceChangeListener { _, v -> current=current.copy(anisotropicFiltering=v.toString().toInt());writeConfig();true }
            })
            findPreference<androidx.preference.PreferenceScreen>("emulator_options")!!.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                key = "show_fps"; title = "Show FPS"; isPersistent = false; isChecked = current.showFps
                summary = "Shows a small counter of rendered frames on the game screen. Applies on the next launch."
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(showFps = value as Boolean)
                    writeConfig()
                    true
                }
            })
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
            if (rootKey != null) preferenceScreen = findPreference<androidx.preference.PreferenceScreen>(rootKey)!!
        }

        override fun onResume() {
            super.onResume()
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

        private fun saveKeyboardBinding(button: String, name: String?) {
            val old=InputBindings(current).keyboard
            val next=org.json.JSONArray()
            var found=false
            for(i in 0 until old.length()) {
                val entry=old.getJSONObject(i)
                if(entry.optString("button")==button) { entry.put("keys",org.json.JSONArray().apply { if(name!=null) put(name) });found=true }
                else if(name!=null) { val keys=entry.optJSONArray("keys") ?: org.json.JSONArray();entry.put("keys",org.json.JSONArray().apply { for(j in 0 until keys.length()) if(!InputBindings.canonical(keys.getString(j)).equals(InputBindings.canonical(name),true)) put(keys.getString(j)) }) }
                next.put(entry)
            }
            if(!found) next.put(JSONObject().put("button",button).put("keys",org.json.JSONArray().apply { if(name!=null) put(name) }))
            current=current.copy(keybindingsJson=next.toString());writeConfig()
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
            val gpuSaved = saved != null && saved.has("gpu_renderer") && saved.optBoolean("gpu_renderer") == current.gpuRenderer && saved.has("anisotropic_filtering") && saved.optInt("anisotropic_filtering") == current.anisotropicFiltering
            if (!gpuSaved) android.widget.Toast.makeText(context, "GPU setting was not saved. Rebuild and install the updated native libraries and APK.", android.widget.Toast.LENGTH_LONG).show()
            if(result?.optBoolean("ok",false)!=true) android.widget.Toast.makeText(context,result?.optString("error") ?: "Could not save settings",android.widget.Toast.LENGTH_LONG).show()
        }
    }
}
