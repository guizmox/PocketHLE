package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class InternalResolutionSettingsTest {
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
