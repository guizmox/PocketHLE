package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class InternalResolutionSettingsTest {
    @Test fun globalFrameLimitAndOptionalGameLimitRemainIndependent() {
        assertEquals("60", LauncherConfig.fromJson(JSONObject()).frameRateLimit)
        assertNull(GameSettings.fromJson(JSONObject()).frameRateLimit)
        for (limit in listOf("10","15","20","30","45","60","120","unlimited")) {
            val global=LauncherConfig.default().copy(frameRateLimit=limit,upscaleFilter="none")
            val restored=LauncherConfig.fromJson(global.toJson())
            assertEquals(limit,restored.frameRateLimit)
            assertEquals("none",restored.upscaleFilter)
            val game=GameSettings.default().copy(frameRateLimit=limit)
            assertEquals(limit,GameSettings.fromJson(game.toJson()).frameRateLimit)
            assertNull(GameSettings.fromJson(game.copy(frameRateLimit=null).toJson()).frameRateLimit)
        }
        val legacy=JSONObject().put("anisotropic_filtering",8).put("frame_rate_limit","unlimited")
        assertEquals("unlimited",GameSettings.fromJson(legacy).frameRateLimit)
        assertEquals("unlimited",LauncherConfig.fromJson(legacy).frameRateLimit)
        assertFalse(GameSettings.fromJson(legacy).toJson().has("anisotropic_filtering"))
        assertFalse(LauncherConfig.fromJson(legacy).toJson().has("anisotropic_filtering"))
    }

    @Test fun allResolutionPoliciesRoundtripWithoutChangingOtherSettings() {
        for (policy in listOf(1,2,3,4,5)) {
            val original=LauncherConfig.default().copy(glesInternalResolution=policy)
            val decoded=LauncherConfig.fromJson(original.toJson())
            assertEquals(policy,decoded.glesInternalResolution)
            assertEquals(original.gpuRenderer,decoded.gpuRenderer)
            assertEquals(original.displayScale,decoded.displayScale)
        }
    }
    @Test fun oldNativeAndAllSizesPreferencesKeepTheirMeaning() {
        assertEquals(1,LauncherConfig.fromJson(JSONObject()).glesInternalResolution)
        assertEquals(2,LauncherConfig.fromJson(JSONObject().put("gles_internal_resolution",2)).glesInternalResolution)
        assertEquals(1,LauncherConfig.fromJson(JSONObject().put("gles_internal_resolution",99)).glesInternalResolution)
    }
}
