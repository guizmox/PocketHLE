package com.pockethle.app
import org.junit.Assert.*
import org.junit.Test

class GizmondoGeometryTest {
    @Test fun normalPreservesTheOriginalBankAndBigReservesItsViewport() {
        for (density in listOf(1f, 2f, 2.625f, 3f)) for ((width, height) in listOf(800 to 360, 915 to 412, 1280 to 800)) {
            val w = (width * density).toInt(); val h = (height * density).toInt()
            val normal = GizmondoGeometry.calculate(w, h, density)
            val big = GizmondoGeometry.calculate(w, h, density, true)
            assertEquals((140 * density).toInt(), normal.dpad.w)
            assertTrue(big.dpad.w > normal.dpad.w)
            assertTrue(big.dpad.w <= normal.dpad.w * 4 / 3 + 2)
            for (g in listOf(normal, big)) {
                assertTrue(g.dpad.x + g.dpad.w < g.screen.x)
                assertTrue(g.dpad.y >= 140 * density)
                assertTrue(g.dpad.y + g.dpad.h <= h)
                assertTrue(g.screen.x + g.screen.w <= w)
                assertTrue(g.screen.y + g.screen.h <= h)
            }
            assertTrue(big.screen.w < normal.screen.w)
        }
    }
    @Test fun shortScreensClampBigInsteadOfCoveringShoulders() {
        val g = GizmondoGeometry.calculate(800, 300, 1f, true)
        assertTrue(g.dpad.w < 186)
        assertTrue(g.dpad.y >= 140)
    }
}
