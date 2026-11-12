package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class InternalResolutionSettingsTest {
    @Test fun gpuCompatibilityProfilePersistsWithoutInheritingRetiredGlobalOptions() {
        assertEquals("generic", GameSettings.fromJson(JSONObject()).gpuCompatibilityProfile)
        for (profile in listOf("generic", "nvidia_goforce5500", "intel2700g", "adreno200")) {
            val original = JSONObject().put("gpu_compatibility_profile", profile)
                .put("gpu_renderer", false).put("future_setting", "kept")
            val saved = GameSettings.fromJson(original).copy(screen = "landscape").toJson()
            assertEquals(profile, GameSettings.fromJson(saved).gpuCompatibilityProfile)
            assertFalse(saved.has("gpu_renderer"))
            assertEquals("kept", saved.getString("future_setting"))
        }
        val legacy = JSONObject().put("pocketpc_intel2700g_depth_bias", true)
            .put("pocketpc_nv_combiners", true).put("future_setting", "kept")
        val saved = LauncherConfig.fromJson(legacy).toJson()
        assertFalse(saved.has("pocketpc_intel2700g_depth_bias"))
        assertFalse(saved.has("pocketpc_nv_combiners"))
        assertEquals("kept", saved.getString("future_setting"))
        assertEquals("generic", GameSettings.fromJson(legacy).gpuCompatibilityProfile)
    }

    @Test fun eglFormatProfileDefaultsOffAndRoundTripsWithoutChangingOtherSettings() {
        assertFalse(GameSettings.fromJson(JSONObject()).pocketpcEglRgba8888Only)
        val original=JSONObject().put("pocketpc_gapi_columns",true).put("gpu_renderer",false).put("future_setting","kept")
        for (enabled in listOf(false,true)) {
            val saved=GameSettings.fromJson(original).copy(pocketpcEglRgba8888Only=enabled).toJson()
            val restored=GameSettings.fromJson(saved)
            assertEquals(enabled,restored.pocketpcEglRgba8888Only)
            assertTrue(restored.pocketpcGapiColumns)
            assertFalse(saved.has("gpu_renderer"))
            assertEquals("kept",saved.getString("future_setting"))
        }
    }

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
        val policies = listOf(
            Triple(1, 1, false), Triple(2, 2, true), Triple(3, 2, false),
            Triple(4, 3, false), Triple(5, 4, false), Triple(6, 3, true),
            Triple(7, 4, true),
        )
        for ((policy, scale, allSizes) in policies) {
            val original = LauncherConfig.default().withGlesResolution(scale, allSizes)
            val decoded=LauncherConfig.fromJson(original.toJson())
            assertEquals(policy, decoded.glesInternalResolution)
            assertEquals(scale, decoded.glesResolutionScale)
            assertEquals(allSizes, decoded.glesResolutionAllSizes)
            assertEquals(original.gpuRenderer,decoded.gpuRenderer)
        }
    }
    @Test fun removedDisplayScaleIsDiscardedWithoutLosingOtherPreferences() {
        val legacy=JSONObject().put("android_display_scale",1).put("gles_internal_resolution",4).put("future_setting","kept")
        val saved=LauncherConfig.fromJson(legacy).toJson()
        assertFalse(saved.has("android_display_scale"))
        assertEquals(4,saved.getInt("gles_internal_resolution"))
        assertEquals("kept",saved.getString("future_setting"))
    }
    @Test fun oldNativeAndAllSizesPreferencesKeepTheirMeaning() {
        assertEquals(1,LauncherConfig.fromJson(JSONObject()).glesInternalResolution)
        assertEquals(2,LauncherConfig.fromJson(JSONObject().put("gles_internal_resolution",2)).glesInternalResolution)
        assertEquals(1,LauncherConfig.fromJson(JSONObject().put("gles_internal_resolution",99)).glesInternalResolution)
    }
}
