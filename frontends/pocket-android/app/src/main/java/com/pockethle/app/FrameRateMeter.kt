package com.pockethle.app

/** One GL-thread writer, UI-thread readers. Repaints do not count as new frames. */
internal class PresentedFrameCounter {
    private var lastFrame: Any? = null
    @Volatile var total: Long = 0L
        private set

    fun presented(frame: Any) {
        if (lastFrame !== frame) {
            lastFrame = frame
            total++
        }
    }
}

/** Monotonic one-second samples; call reset on resume to exclude background time. */
internal class FrameRateMeter {
    private var startedAt: Long? = null
    private var firstCount = 0L

    fun reset(total: Long, nowNanos: Long) {
        firstCount = total
        startedAt = nowNanos
    }

    fun sample(total: Long, nowNanos: Long): Double? {
        val start = startedAt
        if (start == null || total < firstCount || nowNanos < start) {
            reset(total, nowNanos)
            return null
        }
        val elapsed = nowNanos - start
        if (elapsed < 1_000_000_000L) return null
        val fps = (total - firstCount).toDouble() * 1_000_000_000.0 / elapsed
        reset(total, nowNanos)
        return fps
    }
}
