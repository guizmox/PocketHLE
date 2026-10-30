package com.pockethle.app

import kotlin.math.abs
import kotlin.math.hypot
import org.junit.Assert.*
import org.junit.Test

class PocketPcControllerGeometryTest {
    private fun overlaps(a: ControlRect, b: ControlRect) =
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    private fun centreY2(r: ControlRect) = 2 * r.y + r.h

    @Test fun allOrientationsDensitiesAndSizeModesKeepAlignedSeparateTouchTargets() {
        val sizes = listOf(320 to 480, 360 to 800, 412 to 915, 640 to 360, 800 to 360,
            915 to 412, 1280 to 800, 800 to 1280, 1024 to 768, 768 to 1024, 2560 to 1440)
        for (density in listOf(1f, 1.5f, 2f, 2.625f, 3f, 4f)) for (size in sizes) {
            val w = (size.first * density).toInt(); val h = (size.second * density).toInt()
            for (big in listOf(false, true)) {
                val g = PocketPcGeometry.calculate(w, h, density, big, true)
                val zones = g.actions + g.auxiliary + listOf(g.dpad, g.stick, g.screen)
                val context = "$size / $density / Big=$big"
                zones.forEach { r ->
                    assertTrue("positive $context $r", r.w > 0 && r.h > 0)
                    assertTrue("bounds $context $r", r.x >= 0 && r.y >= 0 && r.x + r.w <= w && r.y + r.h <= h)
                }
                for (i in zones.indices) for (j in i + 1 until zones.size)
                    assertFalse("overlap $i/$j $context", overlaps(zones[i], zones[j]))
                val p = listOf(3, 1, 6, 4).map { g.actions[it] }
                assertEquals("P1-P4 line $context", 1, p.map { it.y }.distinct().size)
                assertEquals(1, p.map { it.h }.distinct().size)
                for (index in listOf(2, 7))
                    assertTrue("D-pad/action axis $context", abs(centreY2(g.actions[index]) - centreY2(g.dpad)) <= 1)
                assertTrue(abs((centreY2(g.actions[0]) + centreY2(g.actions[5])) / 2 - centreY2(g.dpad)) <= 1)
                assertTrue("P row safety $context", p[0].y + p[0].h < minOf(g.dpad.y, g.actions[0].y))
                assertTrue(g.stick.w < g.dpad.w)
                assertTrue(g.stick.x + g.stick.w / 2 < g.actions[0].x + g.actions[0].w / 2)
                for (index in listOf(0, 2, 5, 7)) {
                    val r = g.actions[index]
                    val dx = maxOf(0, r.x - (g.stick.x + g.stick.w), g.stick.x - (r.x + r.w))
                    val dy = maxOf(0, r.y - (g.stick.y + g.stick.h), g.stick.y - (r.y + r.h))
                    assertTrue("stick safety $context", hypot(dx.toDouble(), dy.toDouble()) >= p[0].h * .1)
                }
            }
            val normal = PocketPcGeometry.calculate(w, h, density, false, true)
            val big = PocketPcGeometry.calculate(w, h, density, true, true)
            assertTrue(big.dpad.w > normal.dpad.w)
            assertTrue(big.stick.w > normal.stick.w)
            assertTrue(abs(big.dpad.w - normal.dpad.w * 4f / 3f) <= 2)
            assertTrue(abs(big.stick.w - normal.stick.w * 4f / 3f) <= 2)
            normal.actions.zip(big.actions).forEach { (a, b) -> assertEquals(a.w, b.w); assertEquals(a.h, b.h) }
        }
    }

    private data class PhoneSnapshot(val w: Int, val h: Int, val density: Float, val big: Boolean, val rectangles: String)

    @Test fun classicPhoneGeometryKeepsThePreviousExactRectangles() {
        // Captured before this change, including Big and fractional density.
        val snapshots = listOf(
            PhoneSnapshot(800, 360, 1.0f, false, "152,44,496,310|6,6,67,102|79,6,67,102|6,114,67,102|79,114,67,102|654,6,67,102|727,6,67,102|654,114,67,102|727,114,67,102|152,6,64,32|222,6,64,32|292,6,64,32|444,6,64,32|514,6,64,32|584,6,64,32|6,234,120,120|674,234,120,120"),
            PhoneSnapshot(800, 360, 1.0f, true, "170,44,460,310|6,6,67,83|79,6,67,83|6,95,67,83|79,95,67,83|654,6,67,83|727,6,67,83|654,95,67,83|727,95,67,83|170,6,64,32|240,6,64,32|310,6,64,32|426,6,64,32|496,6,64,32|566,6,64,32|6,196,158,158|636,196,158,158"),
            PhoneSnapshot(360, 800, 1.0f, false, "0,56,360,500|6,562,82,44|94,562,82,44|6,612,82,44|94,612,82,44|182,562,82,44|270,562,82,44|182,612,82,44|270,612,82,44|6,6,51,44|63,6,51,44|120,6,51,44|189,6,51,44|246,6,51,44|303,6,51,44|6,674,120,120|234,674,120,120"),
            PhoneSnapshot(360, 800, 1.0f, true, "0,56,360,462|6,524,82,44|94,524,82,44|6,574,82,44|94,574,82,44|182,524,82,44|270,524,82,44|182,574,82,44|270,574,82,44|6,6,51,44|63,6,51,44|120,6,51,44|189,6,51,44|246,6,51,44|303,6,51,44|6,636,158,158|196,636,158,158"),
            PhoneSnapshot(2401, 1081, 2.625f, false, "397,114,1607,952|15,15,176,314|206,15,176,314|15,344,176,314|206,344,176,314|2019,15,176,314|2210,15,176,314|2019,344,176,314|2210,344,176,314|397,15,168,84|580,15,168,84|763,15,168,84|1470,15,168,84|1653,15,168,84|1836,15,168,84|15,706,360,360|2026,706,360,360"),
            PhoneSnapshot(2401, 1081, 2.625f, true, "505,114,1391,952|15,15,176,257|206,15,176,257|15,287,176,257|206,287,176,257|2019,15,176,257|2210,15,176,257|2019,287,176,257|2210,287,176,257|505,15,168,84|688,15,168,84|871,15,168,84|1362,15,168,84|1545,15,168,84|1728,15,168,84|15,591,475,475|1911,591,475,475")
        )
        snapshots.forEach { fixture ->
            val g = PocketPcGeometry.calculate(fixture.w, fixture.h, fixture.density, fixture.big)
            val actual = (listOf(g.screen) + g.actions + g.auxiliary + listOf(g.dpad, g.stick))
                .joinToString("|") { "${it.x},${it.y},${it.w},${it.h}" }
            assertEquals("${fixture.w}x${fixture.h} / ${fixture.big}", fixture.rectangles, actual)
        }
    }

    @Test fun tabletGroupsDoNotStretchAsTheViewportGetsTaller() {
        for (big in listOf(false, true)) {
            val a = PocketPcGeometry.calculate(1280, 800, 1f, big, true)
            val b = PocketPcGeometry.calculate(2560, 1440, 1f, big, true)
            assertEquals(a.dpad.w, b.dpad.w)
            assertEquals(a.stick.w, b.stick.w)
            a.actions.zip(b.actions).forEach { (x, y) -> assertEquals(x.w, y.w); assertEquals(x.h, y.h) }
            assertEquals(a.dpad.y - a.actions[3].y, b.dpad.y - b.actions[3].y)
            assertEquals(a.stick.y - a.actions[6].y, b.stick.y - b.actions[6].y)
        }
    }

    @Test fun classicTabletBanksRemainCompactAndNearTheirThumbControls() {
        for (big in listOf(false, true)) {
            val a = PocketPcGeometry.calculate(1280, 800, 1f, big)
            val b = PocketPcGeometry.calculate(2560, 1440, 1f, big)
            assertTrue(a.actions.all { it.h <= if (big) 84 else 104 })
            assertEquals(a.dpad.y - a.actions[0].y, b.dpad.y - b.actions[0].y)
            for (i in 0..3) assertTrue(abs((a.actions[i].x + a.actions[i].w / 2) - (a.dpad.x + a.dpad.w / 2)) <= 140)
            val portrait = PocketPcGeometry.calculate(800, 1280, 1f, big)
            assertTrue(portrait.actions.all { it.w <= 84 })
        }
    }
}
