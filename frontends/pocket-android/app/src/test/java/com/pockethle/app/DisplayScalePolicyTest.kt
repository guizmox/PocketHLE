package com.pockethle.app
import org.junit.Assert.*
import org.junit.Test

class DisplayScalePolicyTest {
    @Test fun bigControlsCanReduceTheIntegerScaleWithoutChangingAspectRatio() {
        // Keep Normal above the x3 threshold with the current control margins.
        // At 1560 px, Normal has only 956 px of viewport and correctly uses x2.
        val normal = GizmondoGeometry.calculate(1600, 1000, 2f).screen
        val big = GizmondoGeometry.calculate(1600, 1000, 2f, true).screen
        val a = displayRect(normal.w, normal.h, 320, 240, 3)
        val b = displayRect(big.w, big.h, 320, 240, 3)
        assertEquals(960, a.width); assertEquals(640, b.width)
        assertEquals(a.width * 3, a.height * 4)
        assertEquals(b.width * 3, b.height * 4)
    }
    @Test fun pocketPcVgaZoomUsesItsActualProducedPixels() {
        val qvga=pocketPcDisplayRect(2400,1800,320,240,3)
        val vga=pocketPcDisplayRect(2400,1800,640,480,3)
        assertEquals(960,qvga.width);assertEquals(720,qvga.height)
        assertEquals(1920,vga.width);assertEquals(1440,vga.height)
        val doubled=pocketPcDisplayRect(2000,1600,640,480,2)
        assertEquals(1280,doubled.width);assertEquals(960,doubled.height)
        val fitted=pocketPcDisplayRect(1000,800,640,480,2)
        assertEquals(640,fitted.width);assertEquals(480,fitted.height)
        val reduced=displayRect(200,150,320,240,1)
        assertEquals(DisplayRect(0,0,200,150),reduced)
    }
}
