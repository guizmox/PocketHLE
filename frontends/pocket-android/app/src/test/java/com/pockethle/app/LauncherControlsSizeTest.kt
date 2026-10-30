package com.pockethle.app
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class LauncherControlsSizeTest {
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
}
