package com.pockethle.app

import kotlin.math.abs

/** Eight sectors drive only the four existing Gizmondo directional contacts. */
object DpadDirections {
    const val UP = 1
    const val DOWN = 2
    const val LEFT = 4
    const val RIGHT = 8
    // 31.5dp on the 140dp pad: resting the thumb must not press a contact.
    const val DEAD_ZONE_RADIUS = 0.45f

    fun at(x: Float, y: Float, width: Float, height: Float, radius: Float = DEAD_ZONE_RADIUS): Int {
        if (width <= 0f || height <= 0f || x < 0f || y < 0f || x >= width || y >= height) return 0
        val dx = (x - width / 2f) / (width / 2f)
        val dy = (y - height / 2f) / (height / 2f)
        if (dx * dx + dy * dy <= radius * radius) return 0
        // tan(22.5 degrees): equal cardinal and diagonal sectors, no gaps.
        val horizontal = if (abs(dx) >= abs(dy) * 0.41421356f) {
            if (dx < 0f) LEFT else RIGHT
        } else 0
        val vertical = if (abs(dy) >= abs(dx) * 0.41421356f) {
            if (dy < 0f) UP else DOWN
        } else 0
        return horizontal or vertical
    }
}
