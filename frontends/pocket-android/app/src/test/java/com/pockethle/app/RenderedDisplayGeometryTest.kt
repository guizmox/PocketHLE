package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class RenderedDisplayGeometryTest {
    @Test fun allInternalDensitiesUseTheSameLogicalIntegerStepsAndTouchCoordinates() {
        for(native in listOf(320 to 240,640 to 480,240 to 320)) {
            for(turn in listOf(0,90,180,270)) {
                val expected=renderedDisplayRect(1920,1080,native.first,native.second,native.first,native.second,turn,true)
                for(scale in 1..4) {
                    val dense=renderedDisplayRect(1920,1080,native.first,native.second,native.first*scale,native.second*scale,turn,true)
                    assertEquals(expected,dense)
                    for(point in listOf(.1f to .2f,.5f to .5f,.8f to .7f)) {
                        val x=dense.left+dense.width*point.first;val y=dense.top+dense.height*point.second
                        val guest=displayPointToGuest(x,y,1920,1080,native.first,native.second,turn,true)
                        assertEquals(guest,displayPointToGuest(x,y,1920,1080,native.first,native.second,turn,true,false,native.first*scale,native.second*scale))
                    }
                    assertNull(displayPointToGuest(dense.left-1f,dense.top.toFloat(),1920,1080,native.first,native.second,turn,true,false,native.first*scale,native.second*scale))
                }
            }
        }
    }

    @Test fun key2Uses960By720WithoutOverlappingControlsAtEveryInternalDensity() {
        val layout=PocketPcGeometry.calculate(1620,1080,2.4f,false,true)
        val room=layout.screen
        assertEquals(ControlRect(330,69,960,980),room)
        for(scale in 1..4) {
            val fit=renderedDisplayRect(room.w,room.h,320,240,320*scale,240*scale,0,true)
            assertEquals(DisplayRect(0,130,960,720),fit)
            val shown=ControlRect(room.x+fit.left,room.y+fit.top,fit.width,fit.height)
            for(control in layout.actions+layout.auxiliary+listOf(layout.dpad,layout.stick)) {
                assertFalse("game overlaps $control",shown.x<control.x+control.w && control.x<shown.x+shown.w && shown.y<control.y+control.h && control.y<shown.y+shown.h)
            }
            assertEquals(12,shown.x-(layout.actions[1].x+layout.actions[1].w))
            assertEquals(12,layout.actions[6].x-(shown.x+shown.w))
            assertEquals(DisplayRect(0,0,960,720),renderedDisplayRect(960,720,320,240,320*scale,240*scale,0,true))
        }
    }

    @Test fun logicalImagesTooLargeForTheirRoomFitProportionally() {
        for(scale in 1..4) {
            assertEquals(DisplayRect(33,0,533,400),renderedDisplayRect(600,400,640,480,640*scale,480*scale,0,true))
            assertEquals(DisplayRect(0,33,400,533),renderedDisplayRect(400,600,640,480,640*scale,480*scale,90,true))
        }
    }

    @Test fun allFittingIntegerStepsAreAllowedAndNativeVgaKeepsItsOwnGrid() {
        for(step in 1..12) {
            assertEquals(DisplayRect(0,0,320*step,240*step),renderedDisplayRect(320*step,240*step,320,240,1280,960,0,true))
        }
        assertEquals(DisplayRect(160,250,640,480),renderedDisplayRect(960,980,640,480,1280,960,0,true))
        assertEquals(DisplayRect(160,250,640,480),renderedDisplayRect(960,980,320,240,640,480,0,false))
    }

    @Test fun controllerFullscreenRetainsProducedAspectFit() {
        for(turn in listOf(0,90,180,270)) {
            val quarter=turn==90 || turn==270
            assertEquals(aspectDisplayRect(1280,800,if(quarter)480 else 640,if(quarter)640 else 480),renderedDisplayRect(1280,800,320,240,640,480,turn,true,true))
        }
    }
}
