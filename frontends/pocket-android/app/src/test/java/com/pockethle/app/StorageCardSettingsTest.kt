package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class StorageCardSettingsTest {
    @Test fun accelerometerOutputSurvivesSettingsUpdates() {
        for (output in listOf("arrows", "stick")) {
            val settings = GameSettings.fromJson(JSONObject().put("accelerometer_output", output)).copy(screen="landscape")
            assertEquals(output, GameSettings.fromJson(settings.toJson()).accelerometerOutput)
        }
        assertEquals("arrows", GameSettings.fromJson(JSONObject().put("accelerometer_output", "other")).accelerometerOutput)
    }
    @Test fun accelerometerArrowsSurvivesOtherSettingsChangesAndRejectsUnknownModes() {
        for (mode in listOf("disabled", "low", "medium", "high")) {
            val settings = GameSettings.fromJson(JSONObject().put("accelerometer_arrows", mode)).copy(rotation="half")
            assertEquals(mode, GameSettings.fromJson(settings.toJson()).accelerometerArrows)
        }
        assertEquals("disabled", GameSettings.fromJson(JSONObject().put("accelerometer_arrows", "unknown")).accelerometerArrows)
    }

    @org.junit.Test
    fun phoneSoftkeysDefaultOffAndSurviveOtherSettingsChanges() {
        org.junit.Assert.assertFalse(GameSettings.fromJson(org.json.JSONObject()).pocketpcPhoneSoftkeys)
        val incoming=org.json.JSONObject().put("pocketpc_phone_softkeys",true)
        val settings=GameSettings.fromJson(incoming).copy(screen="landscape")
        org.junit.Assert.assertTrue(settings.toJson().getBoolean("pocketpc_phone_softkeys"))
        org.junit.Assert.assertTrue(GameSettings.fromJson(settings.toJson()).pocketpcPhoneSoftkeys)
    }

    @Test fun editingSettingsPreservesPortableCardAndUnlimitedRuntime() {
        val incoming = JSONObject("""{"storage_card_dir":"cards/sd-card","max_slices":0,"host_extension":{"keep":true}}""")
        val settings = GameSettings.fromJson(incoming)
        assertEquals("cards/sd-card", settings.storageCardDir)
        val saved = settings.copy(rotation = "none").toJson()
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
