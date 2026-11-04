package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class StorageCardSettingsTest {
    @Test fun editingSettingsPreservesPortableCardAndUnlimitedRuntime() {
        val incoming = JSONObject("""{"storage_card_dir":"cards/sd-card","max_slices":0,"host_extension":{"keep":true}}""")
        val settings = GameSettings.fromJson(incoming)
        assertEquals("cards/sd-card", settings.storageCardDir)
        val saved = settings.copy(rotation = "none", gpuRenderer = false).toJson()
        assertEquals("cards/sd-card", saved.getString("storage_card_dir"))
        assertEquals(0L, saved.getLong("max_slices"))
        assertTrue(saved.getJSONObject("host_extension").getBoolean("keep"))
    }

    @Test fun oldProfilesKeepDefaultCardSelection() {
        val settings = GameSettings.fromJson(JSONObject("{}"))
        assertNull(settings.storageCardDir)
        assertFalse(settings.toJson().has("storage_card_dir"))
    }
}
