package com.pockethle.app
import org.junit.Assert.*
import org.junit.Test

class DisplayScalePolicyTest {
    @Test fun bigControlsCanReduceTheIntegerScaleWithoutChangingAspectRatio() {
        val normal = GizmondoGeometry.calculate(1560, 1000, 2f).screen
        val big = GizmondoGeometry.calculate(1560, 1000, 2f, true).screen
        val a = displayRect(normal.w, normal.h, 320, 240, 3)
        val b = displayRect(big.w, big.h, 320, 240, 3)
        assertEquals(960, a.width); assertEquals(640, b.width)
        assertEquals(a.width * 3, a.height * 4)
        assertEquals(b.width * 3, b.height * 4)
    }
    @Test fun pocketPcVgaUsesTheExistingQvgaGridAndTooSmallViewportsClip() {
        val qvga = pocketPcDisplayRect(1200, 1200, 320, 240, 3)
        val vga = pocketPcDisplayRect(1200, 1200, 640, 480, 3)
        assertEquals(qvga, vga)
        assertEquals(960, vga.width); assertEquals(720, vga.height)
        val nativeVga = pocketPcDisplayRect(1200, 1200, 640, 480, 2)
        assertEquals(640, nativeVga.width)
        val clipped = displayRect(200, 150, 320, 240, 1)
        assertEquals(320, clipped.width); assertTrue(clipped.left < 0)
    }
}
