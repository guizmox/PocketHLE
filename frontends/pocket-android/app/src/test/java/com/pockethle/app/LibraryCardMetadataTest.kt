package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class LibraryCardMetadataTest {
    @Test fun nativeCardSnapshotSuppliesPlatformWithoutPerCardBridgeCalls() {
        val raw=JSONObject().put("id","card").put("display_name","Game")
            .put("executable","game.exe").put("source_cab","game.zip")
            .put("is_gizmondo",true).put("graphics_api","OpenGL ES").put("custom_settings",true)
        val card=GameEntry.fromJson(raw)
        assertTrue(card.isGizmondo);assertTrue(card.customSettings);assertEquals("OpenGL ES",card.graphicsApi)
        raw.put("is_gizmondo",false).put("custom_settings",false)
        assertFalse(GameEntry.fromJson(raw).isGizmondo)
        assertFalse(GameEntry.fromJson(raw).customSettings)
    }
}
