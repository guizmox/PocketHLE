package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class DisplayedFpsTest {
    @Test fun abandonedFramesAndRepaintsDoNotIncreaseDisplayedRate() {
        val counter = PresentedFrameCounter()
        val meter = FrameRateMeter()
        meter.reset(0L, 0L)
        val submitted = List(400) { Any() }
        for (frame in submitted.takeLast(60)) repeat(10) { counter.presented(frame) }
        assertEquals(60L, counter.total)
        assertEquals(60.0, meter.sample(counter.total, 1_000_000_000L)!!, 0.0001)
        repeat(100) { counter.presented(submitted.last()) }
        assertEquals(0.0, meter.sample(counter.total, 2_000_000_000L)!!, 0.0001)
    }
}
