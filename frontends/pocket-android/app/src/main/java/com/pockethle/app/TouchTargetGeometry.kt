package com.pockethle.app

/** Match an oval drawable, excluding its transparent rectangular corners. */
internal fun ovalTouchContains(x: Float, y: Float, width: Int, height: Int): Boolean {
    if (width <= 0 || height <= 0) return false
    val dx = (x - width / 2f) / (width / 2f)
    val dy = (y - height / 2f) / (height / 2f)
    return dx * dx + dy * dy <= 1f
}
