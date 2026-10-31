package com.pockethle.app
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class LauncherControlsSizeTest {
    @Test fun controllerLayoutAndSizeRoundTripWithoutDroppingOtherSettings() {
        val old = LauncherConfig.fromJson(JSONObject("{\"untouched\":42}"))
        assertEquals("classic", old.pocketpcTouchLayout)
        for (layout in listOf("classic", "controller")) for (size in listOf("normal", "big")) {
            val changed = old.copy(pocketpcTouchLayout = layout, pocketpcControlsSize = size)
            val loaded = LauncherConfig.fromJson(changed.toJson())
            assertEquals(layout, loaded.pocketpcTouchLayout)
            assertEquals(size, loaded.pocketpcControlsSize)
            assertEquals("normal", loaded.gizmondoControlsSize)
            assertEquals(42, loaded.toJson().getInt("untouched"))
        }
        assertEquals("classic", LauncherConfig.fromJson(JSONObject("{\"android_pocketpc_touch_layout\":\"future\"}")).pocketpcTouchLayout)
    }

    @Test fun oldConfigDefaultsAndSizeSettingsRemainIndependent() {
        val old = LauncherConfig.fromJson(JSONObject("{\"optimizations\":{\"fast_memory_access\":false},\"untouched\":42}"))
        assertEquals("normal", old.gizmondoControlsSize)
        assertEquals("normal", old.pocketpcControlsSize)
        val changed = old.copy(gizmondoControlsSize = "big").toJson()
        val reloaded = LauncherConfig.fromJson(changed)
        assertEquals("big", reloaded.gizmondoControlsSize)
        assertEquals("normal", reloaded.pocketpcControlsSize)
        assertEquals(42, reloaded.toJson().getInt("untouched"))
        assertFalse(reloaded.toJson().getJSONObject("optimizations").getBoolean("fast_memory_access"))
    }
    @Test fun unknownSizeFallsBackToNormalInTheAndroidModel() {
        val config = LauncherConfig.fromJson(JSONObject("{\"android_gizmondo_controls_size\":\"future\"}"))
        assertEquals("normal", config.gizmondoControlsSize)
    }
    @Test fun gizmondoOverflowDefaultsOffAndRoundTripsIndependently() {
        val old = LauncherConfig.fromJson(JSONObject("{\"untouched\":42}"))
        assertFalse(old.gizmondoMaximizeScreen)
        for (enabled in listOf(true, false)) for (size in listOf("normal", "big")) {
            val loaded = LauncherConfig.fromJson(old.copy(gizmondoMaximizeScreen = enabled,
                gizmondoControlsSize = size).toJson())
            assertEquals(enabled, loaded.gizmondoMaximizeScreen)
            assertEquals(size, loaded.gizmondoControlsSize)
            assertEquals("normal", loaded.pocketpcControlsSize)
            assertEquals(42, loaded.toJson().getInt("untouched"))
        }
    }
}
