package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class GizmondoGeometryTest {
    @Test fun overflowOnlyChangesTheGameViewport() {
        // Safe landscape viewports: KEY2, P40 Pro+, phone, tablet; also portrait.
        val screens = listOf(Triple(1620, 1080, 1080f / 449f),
            Triple(2486, 1200, 1200f / 362f), Triple(800, 360, 1f),
            Triple(1280, 800, 1f), Triple(1080, 1620, 1080f / 449f))
        for ((w, h, density) in screens) for (big in listOf(false, true)) {
            val reserved = GizmondoGeometry.calculate(w, h, density, big)
            val overlay = GizmondoGeometry.calculate(w, h, density, big, true)
            assertEquals(reserved.copy(screen = overlay.screen), overlay)
            assertEquals(ControlRect(0, 0, w, h), overlay.screen)
            assertTrue(reserved.screen.w < overlay.screen.w)
            for (g in listOf(reserved, overlay)) {
                for (r in listOf(g.dpad) + g.shoulders + g.actions + g.toolbar) {
                    assertTrue(r.x >= 0 && r.y >= 0 && r.w > 0 && r.h > 0)
                    assertTrue(r.x + r.w <= w && r.y + r.h <= h)
                }
                val toolBottom = g.toolbar.maxOf { it.y + it.h }
                assertTrue(g.shoulders.all { it.y >= toolBottom + (3 * density).toInt() })
                val lowerTop = minOf(g.dpad.y, g.actions.minOf { it.y })
                assertTrue(g.shoulders.all { it.y + it.h <= lowerTop - (7 * density).toInt() })
                val axis = g.dpad.y + g.dpad.h / 2f
                assertEquals(axis, g.actions[1].y + g.actions[1].h / 2f, 2f)
                assertTrue(g.toolbar.all { it.w > it.h })
                assertTrue(g.toolbar.all { it.y == (8 * density).toInt() })
            }
            assertTrue(reserved.dpad.x + reserved.dpad.w < reserved.screen.x)
            assertTrue(reserved.screen.x + reserved.screen.w < reserved.actions.minOf { it.x })
        }
    }

    @Test fun bigEnlargesBothThumbGroupsAndShortScreensKeepShouldersSeparate() {
        for ((w, h) in listOf(800 to 360, 915 to 412, 1280 to 800)) {
            val normal = GizmondoGeometry.calculate(w, h, 1f)
            val big = GizmondoGeometry.calculate(w, h, 1f, true)
            assertTrue(big.dpad.w > normal.dpad.w)
            assertTrue(big.dpad.w <= normal.dpad.w * 4 / 3 + 2)
            assertEquals(normal.toolbar, big.toolbar)
            assertTrue(big.actions[0].w > normal.actions[0].w)
            for (g in listOf(normal, big)) {
                assertEquals(44f * g.controlScale, g.actions[0].w.toFloat(), 1f)
                assertTrue(g.shoulders.all { it.y + it.h < g.dpad.y })
            }
        }
    }

    @Test fun constrainedHeightClampsBothGroupsWithoutCoveringTheShoulders() {
        val g = GizmondoGeometry.calculate(800, 240, 1f, true)
        assertTrue(g.dpad.w < 186)
        assertEquals(44f * g.controlScale, g.actions[0].w.toFloat(), 1f)
        assertTrue(g.shoulders.all { it.y + it.h <= g.dpad.y - 8 })
    }

    @Test fun referencePhonesCanFitAtLeastFourTimesTheNativeScreenInBothSizes() {
        for ((w, h, density) in listOf(Triple(1620, 1080, 1080f / 449f),
            Triple(2486, 1200, 1200f / 362f))) for (big in listOf(false, true)) {
            val g = GizmondoGeometry.calculate(w, h, density, big, true)
            assertTrue(minOf(g.screen.w / 320, g.screen.h / 240) >= 4)
        }
    }
}
