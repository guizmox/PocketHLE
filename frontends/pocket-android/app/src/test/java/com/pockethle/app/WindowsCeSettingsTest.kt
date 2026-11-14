package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class WindowsCeSettingsTest {
    @Test fun displayChangesPreserveTheMountedImageAndItsPrivateRegistry() {
        val input = JSONObject("""{"windows_ce":{"image_root":"ce-images/test/fs","data_dir":"ce-data/test","command_line":"\\Application\\","classic_windows":true,"extension":{"keep":7}},"unknown_setting":9}""")
        val saved = GameSettings.fromJson(input).copy(rotation = "cw90").toJson()
        val ce = saved.getJSONObject("windows_ce")
        assertEquals("ce-images/test/fs", ce.getString("image_root"))
        assertEquals("ce-data/test", ce.getString("data_dir"))
        assertEquals("\\Application\\", ce.getString("command_line"))
        assertEquals(7, ce.getJSONObject("extension").getInt("keep"))
        assertEquals(9, saved.getInt("unknown_setting"))
    }
    @Test fun disablingCeDoesNotReenableItFromTheOriginalJson() {
        val settings = GameSettings.fromJson(JSONObject("""{"windows_ce":{"classic_windows":true}}"""))
        assertTrue(settings.copy(windowsCe = null).toJson().isNull("windows_ce"))
        assertNull(GameSettings.fromJson(JSONObject("{}")).windowsCe)
    }
}
