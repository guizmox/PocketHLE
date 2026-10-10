package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject

/**
 * Mirror of `pocket_library::GameEntry` on the Kotlin side. Built by
 * parsing the JSON blob returned from [NativeBridge.listGames].
 */
data class GameEntry(
    val id: String,
    val displayName: String,
    val provider: String?,
    val executable: String,
    val sourceCab: String,
    val importedAt: Long,
    val icon: String?,
    val settings: GameSettings,
) {
    companion object {
        fun fromJson(obj: JSONObject): GameEntry {
            val settingsObj = obj.optJSONObject("settings") ?: JSONObject()
            return GameEntry(
                id = obj.getString("id"),
                displayName = obj.getString("display_name"),
                provider = obj.optString("provider").takeIf { !obj.isNull("provider") && it.isNotEmpty() },
                executable = obj.getString("executable"),
                sourceCab = obj.getString("source_cab"),
                importedAt = obj.optLong("imported_at"),
                icon = obj.optString("icon").takeIf { !obj.isNull("icon") && it.isNotEmpty() },
                settings = GameSettings.fromJson(settingsObj),
            )
        }

        fun listFromJson(json: String): List<GameEntry> {
            val arr = JSONArray(json)
            val out = ArrayList<GameEntry>(arr.length())
            for (i in 0 until arr.length()) {
                out.add(fromJson(arr.getJSONObject(i)))
            }
            return out
        }
    }
}

data class GameSettings(
    val cpuBackend: String, // "stub" or "unicorn"
    val maxSlices: Long,
    val instructionsPerSlice: Long,
    val haltOnUnimplemented: Boolean,
    val screen: String,
    /**
     * Mirrors `pocket_library::RotationPref`: `"none"`, `"cw90"`,
     * `"half"`, `"ccw90"`. Presentation only — the guest still renders
     * at whatever `screen` says, which is what makes it usable for a
     * landscape-designed game (JumpyBall, Asphalt 2) that only draws
     * correctly when it believes it is on a 240x320 portrait panel:
     * keep the guest portrait and turn the *picture* instead.
     */
    val rotation: String,
    val originalJson: String = "{}",
) {
    fun toJson(): JSONObject = JSONObject(originalJson).apply {
        put("cpu_backend", cpuBackend)
        put("max_slices", maxSlices)
        put("instructions_per_slice", instructionsPerSlice)
        put("halt_on_unimplemented", haltOnUnimplemented)
        put("screen", screen)
        put("rotation", rotation)
    }

    companion object {
        fun default(): GameSettings = GameSettings(
            cpuBackend = "unicorn",
            maxSlices = 50_000_000L,
            instructionsPerSlice = 1_000_000L,
            haltOnUnimplemented = false,
            screen = "portrait",
            rotation = "none",
        )

        fun fromJson(obj: JSONObject): GameSettings = GameSettings(
            cpuBackend = obj.optString("cpu_backend", "unicorn"),
            maxSlices = obj.optLong("max_slices", 50_000_000L),
            instructionsPerSlice = obj.optLong("instructions_per_slice", 1_000_000L),
            haltOnUnimplemented = obj.optBoolean("halt_on_unimplemented", false),
            screen = obj.optString("screen", "portrait"),
            rotation = obj.optString("rotation", "none").ifEmpty { "none" },
            originalJson = obj.toString(),
        )
    }
}

data class LauncherConfig(
    val schemaVersion: Int,
    val defaultCpuBackend: String,
    val verbosity: Int,
    val logUnimplementedApis: Boolean = true,
    val lastImportDir: String?,
    val showFps: Boolean,
    val fullscreen: Boolean,
    val fullscreenMode: String,
    val orientation: String,
    /** Mirrors `LauncherConfig::show_backend_log` — whether the
     * in-game status panel ("Backend: Unicorn (ARM)…") is drawn. */
    val showBackendLog: Boolean,
    val bluetoothEnabled: Boolean = false,
    val cameraEnabled: Boolean = false,
    val gpsEnabled: Boolean = false,
    val gpsFixedEnabled: Boolean = false,
    val gpsFixedLatitude: Double = 0.0,
    val gpsFixedLongitude: Double = 0.0,
    val upscaleFilter: String = "reconstruction",
    val displayScale: Int = 0,
    val originalJson: String = "{}",
    val gprsEnabled: Boolean = true,
    val colorsServerUrl: String = "",
    val colorsTerminalId: String = "",
    /** Mirrors `LauncherConfig::controls_opacity`, 0.1..=1.0. */
    val controlsOpacity: Float,
    /**
     * The keybinding list exactly as it came out of `config.json`,
     * carried through untouched.
     *
     * Android has no rebinding UI yet, but the desktop launcher stores
     * its host-key map in the same `config.json`; re-emitting whatever
     * we read keeps an Android settings change from wiping the
     * bindings a user set up on the PC. `null` means the file had none,
     * in which case we leave the key out and let serde's `default`
     * fill it in.
     */
    val keybindingsJson: String?,
) {
    fun toJson(): JSONObject = JSONObject(originalJson).apply {
        put("schema_version", schemaVersion)
        put("default_cpu_backend", defaultCpuBackend)
        put("verbosity", verbosity)
        put("log_unimplemented_apis", logUnimplementedApis)
        if (lastImportDir != null) put("last_import_dir", lastImportDir) else put("last_import_dir", JSONObject.NULL)
        put("show_fps", showFps)
        put("fullscreen", fullscreen)
        put("fullscreen_mode", fullscreenMode)
        put("orientation", orientation)
        put("show_backend_log", showBackendLog)
        put("bluetooth_enabled", bluetoothEnabled)
        put("camera_enabled", cameraEnabled)
        put("gps_enabled", gpsEnabled)
        put("gps_fixed_enabled", gpsFixedEnabled)
        put("gps_fixed_latitude", gpsFixedLatitude)
        put("gps_fixed_longitude", gpsFixedLongitude)
        put("upscale_filter", upscaleFilter)
        put("android_display_scale", displayScale)
        put("gprs_enabled", gprsEnabled)
        put("colors_server_url", colorsServerUrl)
        put("colors_terminal_id", colorsTerminalId)
        put("controls_opacity", controlsOpacity.toDouble())
        if (keybindingsJson != null) {
            runCatching { put("keybindings", JSONArray(keybindingsJson)) }
        }
    }

    companion object {
        fun default(): LauncherConfig = LauncherConfig(
            schemaVersion = 1,
            defaultCpuBackend = "unicorn",
            verbosity = 1,
            lastImportDir = null,
            showFps = true,
            fullscreen = false,
            fullscreenMode = "with_controls",
            orientation = "auto",
            showBackendLog = true,
            controlsOpacity = 1.0f,
            keybindingsJson = null,
        )

        fun fromJson(obj: JSONObject): LauncherConfig = LauncherConfig(
            schemaVersion = obj.optInt("schema_version", 1),
            defaultCpuBackend = obj.optString("default_cpu_backend", "unicorn"),
            verbosity = obj.optInt("verbosity", 1),
            logUnimplementedApis = obj.optBoolean("log_unimplemented_apis", true),
            lastImportDir = obj.optString("last_import_dir").takeIf { !obj.isNull("last_import_dir") && it.isNotEmpty() },
            showFps = obj.optBoolean("show_fps", true),
            fullscreen = obj.optBoolean("fullscreen", false),
            fullscreenMode = obj.optString("fullscreen_mode", "with_controls"),
            orientation = obj.optString("orientation", "auto"),
            showBackendLog = obj.optBoolean("show_backend_log", true),
            bluetoothEnabled = obj.optBoolean("bluetooth_enabled", false),
            cameraEnabled = obj.optBoolean("camera_enabled", false),
            gpsEnabled = obj.optBoolean("gps_enabled", false),
            gpsFixedEnabled = obj.optBoolean("gps_fixed_enabled", false),
            gpsFixedLatitude = obj.optDouble("gps_fixed_latitude", 0.0),
            gpsFixedLongitude = obj.optDouble("gps_fixed_longitude", 0.0),
            upscaleFilter = obj.optString("upscale_filter", "reconstruction"),
            displayScale = obj.optInt("android_display_scale", 0).coerceIn(0,4),
            originalJson = obj.toString(),
            gprsEnabled = obj.optBoolean("gprs_enabled", true),
            colorsServerUrl = obj.optString("colors_server_url", ""),
            colorsTerminalId = obj.optString("colors_terminal_id", ""),
            controlsOpacity = obj.optDouble("controls_opacity", 1.0).toFloat()
                .coerceIn(0.1f, 1.0f),
            keybindingsJson = obj.optJSONArray("keybindings")?.toString(),
        )
    }
}
