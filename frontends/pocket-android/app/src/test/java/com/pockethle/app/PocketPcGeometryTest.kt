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
            else {
                assertTrue(layout.auxiliary.all { it.y+it.h<layout.screen.y })
                assertEquals(1,layout.auxiliary.map { it.y }.distinct().size)
                val guestEnd=layout.auxiliary[2].let { it.x+it.w }
                assertTrue("host action separation",layout.auxiliary[3].x-guestEnd>=18*density-3)
            }
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
    @Test fun vgaUsesQvgaMultiplesAndStillMapsToEveryNativePixel() {
        val rect=pocketPcDisplayRect(1400,800,640,480,0)
        assertEquals(960,rect.width);assertEquals(720,rect.height)
        assertEquals(0 to 0,displayPointToGuest(rect.left.toFloat(),rect.top.toFloat(),1400,800,640,480,0,0,true))
        assertEquals(639 to 479,displayPointToGuest((rect.left+rect.width-1).toFloat(),(rect.top+rect.height-1).toFloat(),1400,800,640,480,0,0,true))
        for(turn in listOf(0,90,180,270)) {
            val w=if(turn%180==0) 640 else 480;val h=if(turn%180==0) 480 else 640
            val fit=pocketPcDisplayRect(1080,1920,w,h,0)
            assertEquals(0,fit.width%(w/2));assertEquals(0,fit.height%(h/2))
            val pos=displayPointToGuest((fit.left+fit.width-1).toFloat(),(fit.top+fit.height-1).toFloat(),1080,1920,640,480,turn,0,true)!!
            assertTrue(pos.first in 0..639);assertTrue(pos.second in 0..479)
        }
        // Gizmondo / old callers retain the original native-pixel scale policy.
        assertEquals(640,displayRect(1400,800,640,480,0).width)
    }

    @Test fun arrowsAndStylusUseTheSameSingleInverseImageTurn() {
        val keys=listOf(0x26,0x27,0x28,0x25)
        val vectors=listOf(0f to -1f,1f to 0f,0f to 1f,-1f to 0f)
        for(turn in 0..3) for(index in keys.indices) {
            val expected=(index-turn+4)%4
            assertEquals(keys[expected],displayDirectionToGuest(keys[index],turn))
            val v=vectors[index]
            val actual=displayVectorToGuest(v.first,v.second,turn)
            assertEquals(vectors[expected].first,actual.first,0f)
            assertEquals(vectors[expected].second,actual.second,0f)
            assertEquals(keys[index],displayDirectionToGuest(displayDirectionToGuest(keys[index],turn),(4-turn)%4))
            assertEquals(0x0d,displayDirectionToGuest(0x0d,turn))
        }
        assertEquals(0x25,displayDirectionToGuest(0x26,1))
    }

}
