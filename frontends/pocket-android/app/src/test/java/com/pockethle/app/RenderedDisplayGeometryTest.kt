package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class RenderedDisplayGeometryTest {
    @Test fun internalResolutionIsTheBaseForDisplayZoom() {
        for (pocketPc in listOf(false,true)) {
            val native=renderedDisplayRect(1920,1440,320,240,320,240,0,2,pocketPc)
            assertEquals(DisplayRect(640,480,640,480),native)
            val rendered=renderedDisplayRect(1920,1440,320,240,640,480,0,2,pocketPc)
            assertEquals(DisplayRect(320,240,1280,960),rendered)
            val fitted=renderedDisplayRect(1600,900,320,240,640,480,0,2,pocketPc)
            assertEquals(DisplayRect(480,210,640,480),fitted)
        }
    }
    @Test fun portraitAndRotationUseRenderedPixelsButTouchStaysLogical() {
        assertEquals(DisplayRect(240,320,960,1280),
            renderedDisplayRect(1440,1920,240,320,480,640,0,2,true))
        for (turn in listOf(0,90,180,270)) {
            val rect=renderedDisplayRect(1920,1920,320,240,640,480,turn,2,true)
            val point=displayPointToGuest(rect.left+rect.width/2f,rect.top+rect.height/2f,
                1920,1920,320,240,turn,2,true,false,640,480)
            val expected=when(turn) {
                90 -> 160 to 119
                180 -> 159 to 119
                270 -> 159 to 120
                else -> 160 to 120
            }
            assertEquals(expected,point)
            assertNull(displayPointToGuest(rect.left-1f,rect.top.toFloat(),1920,1920,
                320,240,turn,2,true,false,640,480))
        }
    }
    @Test fun nativeGamesAndControllerFullscreenKeepTheirPolicies() {
        assertEquals(pocketPcDisplayRect(1920,1440,640,480,3),
            renderedDisplayRect(1920,1440,640,480,640,480,0,3,true))
        for (turn in listOf(0,90,180,270)) {
            val quarter=turn==90 || turn==270
            val rect=renderedDisplayRect(1280,800,320,240,640,480,turn,2,true,true)
            assertEquals(aspectDisplayRect(1280,800,if(quarter)480 else 640,
                if(quarter)640 else 480),rect)
        }
    }
}
