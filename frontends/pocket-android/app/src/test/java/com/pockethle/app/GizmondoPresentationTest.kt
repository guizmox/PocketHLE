package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class GizmondoPresentationTest {
    @Test fun gizmondoCpuAndGlesUseIdenticalFittingIncludingDownscale() {
        for (policy in 1..7) {
            val cpu = gizmondoPresentationSize(320,240,320,240,policy)
            val gles = gizmondoPresentationSize(320,240,cpu.first,cpu.second,policy)
            assertEquals(cpu,gles)
            for ((w,h) in listOf(240 to 180,600 to 400,960 to 720,1080 to 720,1920 to 1080)) {
                for (fullscreen in listOf(false,true)) {
                    val expected=renderedDisplayRect(w,h,320,240,gles.first,gles.second,0,false,fullscreen)
                    val actual=renderedDisplayRect(w,h,320,240,cpu.first,cpu.second,0,false,fullscreen)
                    assertEquals(expected,actual)
                    assertTrue(actual.left>=0 && actual.top>=0)
                    assertTrue(actual.left+actual.width<=w && actual.top+actual.height<=h)
                }
            }
        }
        // An X4 source uses the existing proportional fit on a smaller screen.
        assertEquals(DisplayRect(33,0,533,400),renderedDisplayRect(600,400,320,240,1280,960,0,false))
        assertEquals(640 to 480,gizmondoPresentationSize(640,480,640,480,5))
        assertEquals(2560 to 1920,gizmondoPresentationSize(640,480,640,480,7))
        assertEquals(1280 to 960,gizmondoPresentationSize(320,240,1280,960,3))
    }
}
