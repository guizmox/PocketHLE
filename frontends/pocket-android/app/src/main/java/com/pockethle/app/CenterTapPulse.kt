package com.pockethle.app

/** Host-only single stylus pulse in logical guest coordinates. */
internal class CenterTapPulse {
    companion object {
        const val COMMAND = 0x1000
        const val DURATION_MS = 60L
    }

    private var point: Pair<Int, Int>? = null
    private var due = 0L
    val active
        get() = point != null

    fun press(now: Long, width: Int, height: Int): Pair<Int, Int>? {
        if (active || width <= 0 || height <= 0) return null
        return pressAt(now,width/2,height/2)
    }

    fun pressAt(now: Long, x: Int, y: Int): Pair<Int,Int>? {
        if (active) return null
        due=now+DURATION_MS
        return (x to y).also { point=it }
    }

    fun releaseDue(now: Long): Pair<Int, Int>? = if (active && now >= due) cancel() else null

    fun cancel(): Pair<Int, Int>? = point.also { point = null }
}
