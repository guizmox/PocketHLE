package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class CompactLandscapeGeometryTest {
    @Test fun referenceLandscapeReachesThreeTimesScaleWithoutCoveringTouchscreen() {
        val density = 1080f / 449f
        for (controller in listOf(false, true)) {
            val normal = PocketPcGeometry.calculate(1620, 1080, density, false, controller)
            assertEquals(3, minOf(normal.screen.w / 320, normal.screen.h / 240))
            for (big in listOf(false, true)) {
                val g = PocketPcGeometry.calculate(1620, 1080, density, big, controller)
                for (r in g.actions + g.auxiliary + listOf(g.dpad, g.stick)) {
                    assertFalse("touchscreen overlap: $controller/$big/$r", r.x < g.screen.x + g.screen.w &&
                        r.x + r.w > g.screen.x && r.y < g.screen.y + g.screen.h && r.y + r.h > g.screen.y)
                }
                if (controller) {
                    val c = g.actions[5]
                    // Big can make the stick wider than the action diamond.
                    // Keep its centre aligned where possible, otherwise shift
                    // it inward to retain the right-bank outer margin.
                    val rightKey = g.actions[4]
                    val rightEdge = rightKey.x + rightKey.w
                    val expectedCentre = minOf(c.x + c.w / 2, rightEdge - g.stick.w / 2)
                    assertEquals(expectedCentre, g.stick.x + g.stick.w / 2)
                    assertTrue(g.stick.x + g.stick.w <= rightEdge + 1)
                    assertTrue(g.stick.y > c.y + c.h)
                }
            }
        }
    }
}
