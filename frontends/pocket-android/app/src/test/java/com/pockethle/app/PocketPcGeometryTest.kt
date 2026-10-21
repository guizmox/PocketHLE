package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class PocketPcGeometryTest {
    private fun overlaps(a:ControlRect,b:ControlRect)=a.x<b.x+b.w && b.x<a.x+a.w && a.y<b.y+b.h && b.y<a.y+a.h
    @Test fun phoneRotationKeepsControlsSeparateFromSurfaceAndThumbs() {
        for(density in listOf(1f,2f,2.625f,3f)) for(size in listOf(360 to 800,412 to 915,800 to 360,915 to 412,1280 to 800)) {
            val w=(size.first*density).toInt();val h=(size.second*density).toInt()
            val layout=PocketPcGeometry.calculate(w,h,density)
            val zones=layout.actions+layout.auxiliary+listOf(layout.dpad,layout.stick,layout.screen)
            zones.forEach { r -> assertTrue("positive $r",r.w>0 && r.h>0);assertTrue("bounds $r",r.x>=0 && r.y>=0 && r.x+r.w<=w && r.y+r.h<=h) }
            for(i in zones.indices) for(j in i+1 until zones.size) assertFalse("overlap $i/$j at $size density $density",overlaps(zones[i],zones[j]))
            layout.actions.forEach { assertTrue("thumb safety gap",it.y+it.h+16*density<=layout.dpad.y) }
            if(w>h) assertEquals(layout.actions[0].y,layout.auxiliary[0].y)
            else { assertEquals(0,layout.screen.y); assertTrue(layout.auxiliary.all { it.y>=layout.dpad.y-8*density }) }
        }
    }
    @Test fun nativeIntegerScaleAndTouchMappingSurviveAllPresentationTurns() {
        for(native in listOf(240 to 320,480 to 640)) for(turn in listOf(0,90,180,270)) {
            val quarter=turn==90 || turn==270
            val w=if(quarter) native.second else native.first;val h=if(quarter) native.first else native.second
            for(room in listOf(1080 to 1800,1800 to 800,3840 to 2160)) {
                val rect=displayRect(room.first,room.second,w,h,4)
                assertEquals(0,rect.width%w);assertEquals(0,rect.height%h)
                assertTrue(rect.width/w in 1..4);assertEquals(rect.width/w,rect.height/h)
                val point=displayPointToGuest((rect.left+rect.width/2).toFloat(),(rect.top+rect.height/2).toFloat(),room.first,room.second,native.first,native.second,turn,4)
                assertNotNull(point);assertTrue(point!!.first in 0 until native.first);assertTrue(point.second in 0 until native.second)
            }
        }
    }
}
