package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class StylusGridTest {
    @Test fun latestPressRestoresHeldZoneAndReleases() {
        val g=StylusGridConfig();val a=GridInput("keyboard","A");val b=GridInput("controller","South")
        assertNull(g.bind(0,a,null));assertNull(g.bind(19,b,null));val r=StylusGridRouter(g,null)
        assertEquals(GridTouch(0,1000,1250),r.input(a,true))
        assertNull(r.input(a,true))
        assertEquals(GridTouch(1,9000,8750),r.input(b,true))
        assertEquals(GridTouch(1,1000,1250),r.input(b,false))
        assertEquals(GridTouch(2,1000,1250),r.input(a,false))
        assertFalse(r.active)
        assertFalse(r.assigned(GridInput("keyboard","Z")))
    }
    @Test fun reservedInputsAndConfiguredFullscreenAreRefused() {
        val g=StylusGridConfig()
        for(k in listOf("Escape","F10","F11","F12")) assertNotNull(g.bind(0,GridInput("keyboard",k),null))
        for(k in listOf("Select","Back","Start","Guide","Mode","Misc1")) assertNotNull(g.bind(0,GridInput("controller",k),null))
        assertNotNull(g.bind(0,GridInput("controller","South"),"South"))
        g.bind(0,GridInput("controller","South"),null)
        assertFalse(StylusGridRouter(g,"South").assigned(GridInput("controller","South")))
    }
    @Test fun gridCentersStayInBoundsForEverySizeResolutionAndRotation() {
        for((c,r) in StylusGridConfig.sizes) {
            val g=StylusGridConfig(c,r)
            for(cell in 0 until c*r) {
                val (x,y)=g.point(cell)
                for(turns in 0..3)for((w,h)in listOf(240 to 320,480 to 640,320 to 240,640 to 480)) {
                    val (gx,gy)=StylusGridConfig.guestPoint(x,y,turns,w,h)
                    assertTrue(gx in 0 until w);assertTrue(gy in 0 until h)
                }
            }
        }
        assertEquals(180 to 240,StylusGridConfig.guestPoint(2500,7500,1,240,320))
        assertEquals(60 to 80,StylusGridConfig.guestPoint(2500,7500,3,240,320))
    }
    @Test fun settingsMigrateAndRoundTrip() {
        val old=GameSettings.fromJson(JSONObject().put("pocketpc_touch_steering",true))
        assertEquals("zones",old.controlsMode())
        val g=StylusGridConfig();g.bind(7,GridInput("controller","RightStickLeft"),null)
        val settings=old.copy(pocketpcControls="grid",pocketpcTouchSteering=false,pocketpcStylusGridJson=g.json().toString())
        val saved=GameSettings.fromJson(settings.toJson())
        assertEquals("grid",saved.controlsMode())
        assertEquals(7,StylusGridConfig.parse(saved.pocketpcStylusGridJson).cellFor(GridInput("controller","RightStickLeft"),null))
        assertEquals("normal",GameSettings.default().controlsMode())
    }
    @Test fun replacingBindingsAndCancellationDoNotLeakHeldPresses() {
        val g=StylusGridConfig();val a=GridInput("keyboard","ArrowUp")
        g.bind(0,a,null);g.bind(10,GridInput("keyboard","Up"),null)
        assertEquals(1,g.bindings.size)
        assertEquals(10,g.cellFor(a,null))
        val r=StylusGridRouter(g,null);r.input(a,true)
        assertNotNull(r.cancel());assertNull(r.cancel());assertFalse(r.active)
        assertNull(r.input(a,false))
    }
}
