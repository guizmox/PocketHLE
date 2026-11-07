package com.pockethle.app

/** Matches pocket-library::stylus::StylusPointer. No guest input before click disambiguation. */
internal class StylusPointer {
    var pointer = false
        private set

    private var point: Pair<Float, Float>? = null
    private var releasedAt: Long? = null
    private var tapPosition: Pair<Int, Int>? = null
    private var held = false
    private var second = false

    fun button(down: Boolean, now: Long): Boolean {
        if (down) {
            if (held) return false
            held = true
            second = releasedAt?.let { now - it < DOUBLE_MS } == true
            if (second) releasedAt = null
        } else if (held) {
            held = false
            if (second) {
                second = false
                pointer = !pointer
                return true
            }
            releasedAt = now
            tapPosition = position()
        }
        return false
    }

    fun advance(velocity: Pair<Int, Int>, seconds: Float, width: Int, height: Int) {
        val w = width.coerceAtLeast(1)
        val h = height.coerceAtLeast(1)
        val p = point ?: (w / 2f to h / 2f)
        val dt = if (pointer) seconds.coerceIn(0f, .05f) else 0f
        point =
            (p.first + velocity.first * dt).coerceIn(0f, (w - 1).toFloat()) to
                (p.second + velocity.second * dt).coerceIn(0f, (h - 1).toFloat())
    }

    fun position() =
        point?.let { kotlin.math.round(it.first).toInt() to kotlin.math.round(it.second).toInt() }

    fun takeTap(now: Long): Pair<Int, Int>? {
        if (releasedAt?.let { now - it >= DOUBLE_MS } == true) {
            releasedAt = null
            if (pointer) return tapPosition.also { tapPosition = null }
        }
        return null
    }

    fun cancelClick() {
        releasedAt = null
        tapPosition = null
        held = false
        second = false
    }

    companion object {
        const val DOUBLE_MS = 300L
    }
}
