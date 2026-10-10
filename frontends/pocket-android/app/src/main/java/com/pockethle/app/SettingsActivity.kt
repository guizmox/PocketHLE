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
            fun toggle(keyName: String, label: String, checked: Boolean, update: (Boolean) -> Unit) {
                giz.addPreference(SwitchPreferenceCompat(requireContext()).apply {
                    key = keyName; title = label; isPersistent = false; isChecked = checked
                    setOnPreferenceChangeListener { _, value -> update(value as Boolean); writeConfig(); true }
                })
            }
            toggle("gprs_enabled", "GPRS/data", current.gprsEnabled) { current = current.copy(gprsEnabled = it) }
            toggle("gps_fixed_enabled", "Fixed GPS position", current.gpsFixedEnabled) { current = current.copy(gpsFixedEnabled = it) }
            fun coordinate(keyName: String, label: String, latitude: Boolean) {
                giz.addPreference(androidx.preference.EditTextPreference(requireContext()).apply {
                    key = keyName; title = label; isPersistent = false
                    text = (if (latitude) current.gpsFixedLatitude else current.gpsFixedLongitude).toString()
                    summaryProvider = androidx.preference.EditTextPreference.SimpleSummaryProvider.getInstance()
                    setOnBindEditTextListener { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL or android.text.InputType.TYPE_NUMBER_FLAG_SIGNED }
                    setOnPreferenceChangeListener { _, value ->
                        val number = value.toString().trim().replace(',', '.').toDoubleOrNull()
                        val limit = if (latitude) 90.0 else 180.0
                        if (number == null || !number.isFinite() || number !in -limit..limit) {
                            android.widget.Toast.makeText(context, "Invalid coordinate (−$limit to $limit)", android.widget.Toast.LENGTH_LONG).show(); false
                        } else {
                            current = if (latitude) current.copy(gpsFixedLatitude = number) else current.copy(gpsFixedLongitude = number)
                            writeConfig(); true
                        }
                    }
                })
            }
            coordinate("gps_fixed_latitude", "Latitude", true)
            coordinate("gps_fixed_longitude", "Longitude", false)
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
                entries = arrayOf("Auto (largest integer scale that fits)", "Native ×1", "×2", "×3", "×4")
                entryValues = arrayOf("0", "1", "2", "3", "4"); value = current.displayScale.toString()
                summary = "Whole multiples of the native resolution. Larger scales are limited to the available space."
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
                summary = "Real device location; fixed position works independently of this option."
                isPersistent = false
                isChecked = current.gpsEnabled
                setOnPreferenceChangeListener { _, value ->
                    current = current.copy(gpsEnabled = value as Boolean)
                    writeConfig(); true
                }
            })
            giz.addPreference(androidx.preference.EditTextPreference(requireContext()).apply {
                key = "colors_server_url"
                title = "Colors multiplayer server"
                summary = "Host, IP or URL, for example nas.local:8080. Applied on the next launch."
                isPersistent = false
                text = current.colorsServerUrl
                setOnPreferenceChangeListener { _, value ->
                    val input = value.toString().trim()
                    val origin = if (input.isEmpty() || input.contains("://")) input else "http://$input"
                    val uri = android.net.Uri.parse(origin)
                    if (origin.isNotEmpty() && (uri.scheme !in listOf("http", "https") || uri.host.isNullOrBlank() || !uri.userInfo.isNullOrBlank() || !uri.query.isNullOrBlank() || !uri.fragment.isNullOrBlank() || (uri.path ?: "") !in listOf("", "/"))) {
                        android.widget.Toast.makeText(context, "Enter a host or HTTP(S) origin without a path", android.widget.Toast.LENGTH_LONG).show(); false
                    } else { current = current.copy(colorsServerUrl = origin.trimEnd('/')); writeConfig(); true }
                }
            })
            giz.addPreference(androidx.preference.EditTextPreference(requireContext()).apply {
                key = "colors_terminal_id"
                title = "Colors player ID (optional)"
                summary = "Leave empty to keep this installation's identity"
                isPersistent = false
                text = current.colorsTerminalId
                setOnPreferenceChangeListener { _, value ->
                    val identity=value.toString().trim()
                    if(identity.length>128 || !identity.matches(Regex("[A-Za-z0-9_.-]*"))) { android.widget.Toast.makeText(context,"ID: ASCII letters, digits, hyphen, dot or underscore (maximum 128 characters)",android.widget.Toast.LENGTH_LONG).show();false }
                    else { current=current.copy(colorsTerminalId=identity);writeConfig();true }
                }
            })
            val input = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="input_options";title="Keyboard and controllers" }
            preferenceScreen.addPreference(input)
            val keyboard = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="keyboard_options";title="Physical keyboard" }
            val gamepad = androidx.preference.PreferenceScreen(requireContext(), null).apply { key="gamepad_options";title="Physical controller" }
            input.addPreference(keyboard);input.addPreference(gamepad)
            InputBindings.buttons.forEach { (button,info) ->
                keyboard.addPreference(Preference(requireContext()).apply {
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
                gamepad.addPreference(ListPreference(requireContext()).apply {
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
            if(result?.optBoolean("ok",false)!=true) android.widget.Toast.makeText(context,result?.optString("error") ?: "Could not save settings",android.widget.Toast.LENGTH_LONG).show()
        }
    }
}
