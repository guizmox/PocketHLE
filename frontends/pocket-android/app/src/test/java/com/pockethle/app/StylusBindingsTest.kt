package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class StylusBindingsTest {
    @Test fun defaultsAreSeparateAndUseOklmAndRightStick() {
        val bindings=StylusBindings(LauncherConfig.default())
        assertEquals(0,bindings.key("O"));assertEquals(2,bindings.key("k"))
        assertEquals(1,bindings.key("L"));assertEquals(3,bindings.key("M"))
        assertEquals(3,bindings.pad("RightStickRight"))
        assertNull(bindings.pad("LeftStickRight"))
    }
    @Test fun oppositeDirectionsCancelAndRotationTracksDisplayedAxes() {
        val b=StylusBindings(LauncherConfig.default())
        assertEquals(0 to 0,b.velocity(floatArrayOf(1f,1f,0f,0f),0))
        assertEquals(0 to -240,b.velocity(floatArrayOf(0f,0f,0f,1f),1))
        assertEquals(-240 to 0,b.velocity(floatArrayOf(0f,0f,0f,1f),2))
        val diagonal=b.velocity(floatArrayOf(1f,0f,0f,1f),0)
        assertTrue(kotlin.math.abs(kotlin.math.hypot(diagonal.first.toDouble(),diagonal.second.toDouble())-240.0)<1.0)
    }
    @Test fun explicitUnboundAndDisabledSurviveConfigRoundTrip() {
        val data=JSONObject().put("enabled",false).put("speed",1200)
            .put("keyboard",JSONArray().apply { repeat(4) { put(JSONArray()) } })
            .put("controller",JSONArray().apply { repeat(4) { put(JSONArray()) } })
        val saved=LauncherConfig.default().copy(originalJson=JSONObject().put("stylus_stick",data).toString())
        val loaded=LauncherConfig.fromJson(saved.toJson())
        val b=StylusBindings(loaded)
        assertFalse(b.enabled);assertEquals(1200,b.speed);assertNull(b.key("O"))
        assertNull(b.pad("RightStickUp"));assertEquals(0 to 0,b.velocity(floatArrayOf(1f,0f,0f,0f),0))
    }
}
