package com.pockethle.app

/** Host-only geometry shared by the control bank and its reserved game viewport. */
internal data class GizmondoGeometry(val dpad: ControlRect, val screen: ControlRect, val toolbarScale: Float) {
    companion object {
        fun calculate(width: Int, height: Int, density: Float, big: Boolean = false): GizmondoGeometry {
            val w = width.coerceAtLeast(1); val h = height.coerceAtLeast(1)
            fun dp(value: Float) = (value * density).toInt().coerceAtLeast(1)
            // Preserve the original 140dp bank in Normal. Big must leave the
            // shoulder button above it and a usable central game viewport.
            val desired = dp(if (big) 140f * 4f / 3f else 140f)
            val vertical = (h - dp(148f)).coerceAtLeast(1)
            val horizontal = (w - dp(308f)).coerceAtLeast(1)
            val size = minOf(desired, vertical, horizontal)
            val left = (size + dp(8f)).coerceAtMost(w - 1)
            val right = dp(148f).coerceAtMost((w - left - 1).coerceAtLeast(0))
            val top = dp(60f).coerceAtMost(h - 1)
            val room = (w - left - right).coerceAtLeast(1)
            return GizmondoGeometry(ControlRect(dp(4f), (h - dp(8f) - size).coerceAtLeast(0), size, size),
                ControlRect(left, top, room, (h - top).coerceAtLeast(1)),
                (room / density / 416f).coerceIn(0.1f, 1f))
        }
    }
}
