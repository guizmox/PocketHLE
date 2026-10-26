package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class ControllerFullscreenGeometryTest {
    private fun overlaps(a:ControlRect,b:ControlRect)=a.x<b.x+b.w && b.x<a.x+a.w && a.y<b.y+b.h && b.y<a.y+a.h
    @Test fun gameAndExitStayInsideTheWindowWithoutOverlapping() {
        for(size in listOf(2400 to 1080,1080 to 2400,1280 to 800,800 to 1280,1080 to 1440,1440 to 1080))
            for(native in listOf(320 to 240,240 to 320,640 to 480,480 to 640))
                for(density in listOf(1f,2.625f,3f)) {
                    val g=controllerFullscreenGeometry(size.first,size.second,native.first,native.second,density)
                    for(r in listOf(g.screen,g.exit)) {
                        assertTrue(r.w>0 && r.h>0)
                        assertTrue(r.x>=0 && r.y>=0 && r.x+r.w<=size.first && r.y+r.h<=size.second)
                    }
                    assertFalse("$size $native $density",overlaps(g.screen,g.exit))
                    assertTrue(kotlin.math.abs(g.screen.w.toLong()*native.second-g.screen.h.toLong()*native.first)<=maxOf(native.first,native.second))
                }
    }
    @Test fun landscapeUsesHeightAndPortraitUsesWidthWhenMarginsAllowIt() {
        val landscape=controllerFullscreenGeometry(2400,1080,320,240,3f)
        assertEquals(1080,landscape.screen.h)
        val portrait=controllerFullscreenGeometry(1080,2400,240,320,3f)
        assertEquals(1080,portrait.screen.w)
    }
    @Test fun fractionalFullscreenInputUsesTheSameRectangleAfterRotation() {
        for(turn in listOf(0,90,180,270)) {
            val w=if(turn%180==0) 240 else 320;val h=if(turn%180==0) 320 else 240
            val rect=aspectDisplayRect(1013,731,w,h)
            assertNotNull(displayPointToGuest(rect.left+rect.width/2f,rect.top+rect.height/2f,1013,731,240,320,turn,1,true,true))
            assertNull(displayPointToGuest(-1f,-1f,1013,731,240,320,turn,1,true,true))
            val centre=displayPointToGuest(rect.left+rect.width/2f,rect.top+rect.height/2f,1013,731,240,320,turn,1,true,true)!!
            assertTrue(centre.first in 118..121 && centre.second in 158..161)
        }
    }
}
