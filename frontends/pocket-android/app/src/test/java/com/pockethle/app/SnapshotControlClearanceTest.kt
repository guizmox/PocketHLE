package com.pockethle.app

import kotlin.math.hypot
import org.junit.Assert.assertTrue
import org.junit.Test

class SnapshotControlClearanceTest {
    @Test fun landscapeControlBanksLeaveAirAroundBothPhysicalCornerButtons() {
        for ((width,height) in listOf(640 to 360,915 to 412,1280 to 800)) {
            for (density in listOf(1f,2f,3.5f)) for (controller in listOf(false,true)) {
                for (big in listOf(false,true)) {
                    val w=(width*density).toInt();val h=(height*density).toInt()
                    val geometry=PocketPcGeometry.calculate(w,h,density,big,controller)
                    val safe=SnapshotControlGeometry.clearance(density).toDouble()
                    for (rect in geometry.actions+geometry.auxiliary+listOf(geometry.dpad,geometry.stick)) {
                        assertTrue("Save overlap: $width/$height/$density/$controller/$big/$rect",
                            hypot(rect.x.toDouble(),rect.y.toDouble())>=safe)
                        assertTrue("Load overlap: $width/$height/$density/$controller/$big/$rect",
                            hypot((w-rect.x-rect.w).toDouble(),rect.y.toDouble())>=safe)
                    }
                }
            }
        }
    }
}
