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

    @Test fun presentationDropsToThePreviousWholeQvgaStep() {
        for(scale in 1..4) {
            assertEquals(DisplayRect(140,80,320,240),renderedDisplayRect(600,400,640,480,640*scale,480*scale,0,true))
            assertEquals(DisplayRect(80,140,240,320),renderedDisplayRect(400,600,640,480,640*scale,480*scale,90,true))
        }
    }

    @Test fun allFittingIntegerStepsAreAllowedForNativeVgaToo() {
        for(step in 1..12) {
            assertEquals(DisplayRect(0,0,320*step,240*step),renderedDisplayRect(320*step,240*step,320,240,1280,960,0,true))
        }
        assertEquals(DisplayRect(0,130,960,720),renderedDisplayRect(960,980,640,480,1280,960,0,true))
        assertEquals(DisplayRect(160,250,640,480),renderedDisplayRect(960,980,320,240,640,480,0,false))
    }

    @Test fun phoneLayoutsFitTheLargestQvgaStepWithoutTouchOverlap() {
        val viewports=listOf(Triple(1080,1620,1080f/449f),Triple(1620,1080,1080f/449f),
            Triple(1200,2488,3.5f),Triple(1080,1540,1080f/449f))
        for ((width,height,density) in viewports) for (big in listOf(false,true)) {
            val layout=PocketPcGeometry.calculate(width,height,density,big,true)
            val room=layout.screen
            for ((nw,nh) in listOf(320 to 240,640 to 480,240 to 320,480 to 640)) for (scale in 1..4) {
                val fit=renderedDisplayRect(room.w,room.h,nw,nh,nw*scale,nh*scale,0,true)
                val sw=if(nw>nh)320 else 240; val sh=if(nw>nh)240 else 320
                val steps=minOf(room.w/sw,room.h/sh)
                assertTrue(steps>=1)
                assertEquals(sw*steps,fit.width);assertEquals(sh*steps,fit.height)
                if(width==1620 && !big && nw>nh) assertEquals(960,fit.width)
                if(width==1080 && height==1620 && !big && nw<nh) assertEquals(960,fit.height)
                val image=ControlRect(room.x+fit.left,room.y+fit.top,fit.width,fit.height)
                for (control in layout.actions+layout.auxiliary+listOf(layout.dpad,layout.stick)) {
                    assertFalse("image=$image control=$control big=$big",image.x<control.x+control.w && control.x<image.x+image.w && image.y<control.y+control.h && control.y<image.y+image.h)
                }
            }
            if(width<height) {
                assertEquals(0,room.y)
                assertTrue(layout.auxiliary.all { it.y>=layout.stick.y+layout.stick.h })
                assertTrue(layout.stick.y+layout.stick.h/2>layout.dpad.y+layout.dpad.h/2)
                if(!big) assertTrue("compact stagger",layout.stick.y+layout.stick.h/2-(layout.dpad.y+layout.dpad.h/2)<layout.dpad.h)
            }
        }
    }

    @Test fun controllerFullscreenRetainsProducedAspectFit() {
        for(turn in listOf(0,90,180,270)) {
            val quarter=turn==90 || turn==270
            assertEquals(aspectDisplayRect(1280,800,if(quarter)480 else 640,if(quarter)640 else 480),renderedDisplayRect(1280,800,320,240,640,480,turn,true,true))
        }
    }
}
