package com.pockethle.app

/** One geometry for painting, touch targets and the game's available viewport. */
internal data class GizmondoGeometry(
    val dpad: ControlRect,
    val screen: ControlRect,
    val toolbarScale: Float,
    val shoulders: List<ControlRect>,
    val actions: List<ControlRect>,
    val toolbar: List<ControlRect>,
    val controlScale: Float,
) {
    companion object {
        fun calculate(width: Int, height: Int, density: Float, big: Boolean = false,
                      maximizeScreen: Boolean = false): GizmondoGeometry {
            val w = width.coerceAtLeast(1); val h = height.coerceAtLeast(1)
            val dp = density.coerceAtLeast(0.1f)
            // Toolbar size is independent of the control size and overflow option.
            val toolbarScale = ((w / dp - 296f) / 416f).coerceIn(0.1f, 1f)
            val unit = dp * toolbarScale
            val start = (w - 416f * unit) / 2f
            fun rect(x: Float, y: Float, rw: Float, rh: Float) = ControlRect(
                x.toInt(), y.toInt(), rw.toInt().coerceAtLeast(1), rh.toInt().coerceAtLeast(1))
            val toolbar = (0..4).map { rect(start + it * 52f * unit, 8f * dp, 44f * unit, 32f * unit) } +
                listOf(rect(start + 280f * unit, 8f * dp, 64f * unit, 32f * unit),
                    rect(start + 352f * unit, 8f * dp, 64f * unit, 32f * unit))
            val shoulderTop = 8f * dp + 32f * unit + 4f * dp
            // Keep both thumb groups on screen, including short and narrow displays.
            val factor = minOf(if (big) 4f / 3f else 1f,
                ((w / dp - 36f) / 276f).coerceAtLeast(0.1f),
                ((h - shoulderTop - 64f * dp) / (140f * dp)).coerceAtLeast(0.1f))
            val size = (140f * dp * factor).toInt().coerceAtLeast(1)
            val scale = size / (140f * dp)
            val pad = ControlRect((4f * dp).toInt(), (h - 8f * dp - size).toInt(), size, size)
            val axis = pad.y + size / 2f
            val diameter = 44f * dp * scale; val delta = 46f * dp * scale
            val faceX = w - 6f * dp - delta - diameter / 2f
            val actions = listOf(0f to -1f, -1f to 0f, 1f to 0f, 0f to 1f).map { (x, y) ->
                rect(faceX + x * delta - diameter / 2f, axis + y * delta - diameter / 2f, diameter, diameter)
            }
            val lowerTop = minOf(pad.y, actions.minOf { it.y }).toFloat()
            val shoulderHeight = minOf(132f * dp, lowerTop - 8f * dp - shoulderTop).coerceAtLeast(1f)
            val shoulderWidth = minOf(124f * dp, size.toFloat())
            val shoulders = listOf(pad.x + size / 2f, faceX).map {
                rect(it - shoulderWidth / 2f, shoulderTop, shoulderWidth, shoulderHeight)
            }
            val left = (maxOf(pad.x + pad.w, shoulders[0].x + shoulders[0].w) + 8f * dp).toInt().coerceIn(0, w - 1)
            val right = (minOf(actions.minOf { it.x }, shoulders[1].x) - 8f * dp).toInt().coerceIn(left + 1, w)
            val top = (8f * dp + 32f * unit + 8f * dp).toInt().coerceIn(0, h - 1)
            val screen = if (maximizeScreen) ControlRect(0, 0, w, h)
                else ControlRect(left, top, right - left, h - top)
            return GizmondoGeometry(pad, screen, toolbarScale, shoulders, actions, toolbar, scale)
        }
    }
}
