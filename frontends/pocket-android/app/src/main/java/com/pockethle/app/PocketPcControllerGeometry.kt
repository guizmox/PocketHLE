package com.pockethle.app

import kotlin.math.roundToInt

/** Physical-size control groups anchored near the thumbs, never stretched with
 * tablet height. Only host geometry changes; all existing input IDs are reused. */
internal object PocketPcControllerGeometry {
    fun calculate(width: Int, height: Int, density: Float, big: Boolean): PocketPcGeometry {
        val w = width.coerceAtLeast(1); val h = height.coerceAtLeast(1)
        val portrait = w <= h
        val pWidth = if (portrait) 76f else 94f
        val normalPad = if (portrait) 120f else 156f
        val normalStick = if (portrait) 80f else 100f
        val maxPad = normalPad * 4f / 3f
        val maxStick = normalStick * 4f / 3f
        val diamondDiameter = 64f
        // Oval action buttons reject DOWN events outside their visible circle.
        // Their bounding rectangles can overlap at empty corners safely.
        val diamondOffset = if (portrait) 70f else 50f
        val stickDrop = if (portrait) 112f else 140f
        val edge = if (portrait) 16f else 8f
        val gutter = if (portrait) 16f else 6f
        val groupWidth = pWidth * 2f + 12f
        val leftWidth = maxOf(groupWidth, maxPad)
        val rightWidth = if (portrait) diamondOffset + diamondDiameter + maxStick + 35f
            else diamondOffset + diamondDiameter / 2f + maxStick
        val minWidth = 2f * edge + leftWidth + rightWidth + if (portrait) 20f else 160f
        // Landscape P keys can sit beside the central toolbar. Keeping this
        // bound equal to the previous Big bound preserves the control sizes.
        val minHeight = (if (portrait) 54f else 26f) + 76f + 24f + maxOf(maxPad / 2f, diamondOffset + 32f) +
            stickDrop + maxStick / 2f + 16f + if (portrait) 120f else 0f
        // Use the same fit scale for Normal and Big. P keys and action buttons
        // therefore retain exactly the same dimensions when size mode changes.
        val unit = minOf(density.coerceAtLeast(.1f) * .8f, w / minWidth, h / minHeight)
        fun px(value: Float) = (value * unit).roundToInt().coerceAtLeast(1)
        val margin = px(edge); val pW = px(pWidth); val pH = px(76f)
        val pGap = px(12f); val safety = px(24f)
        val pad = px(normalPad * if (big) 4f / 3f else 1f)
        val stick = px(normalStick * if (big) 4f / 3f else 1f)
        val diameter = px(diamondDiameter); val delta = px(diamondOffset)
        val faceX = w - margin - delta - diameter / 2
        val stickX = if (portrait) faceX - stick / 2 - diameter / 2 - px(35f)
            else faceX - stick / 2
        val stickY = h - px(16f) - stick / 2
        // One shared horizontal axis for the D-pad and all four action keys.
        val axisY = stickY - px(stickDrop)
        val pY = axisY - maxOf(pad / 2, delta + diameter / 2) - safety - pH
        val pLeft1 = ControlRect(margin, pY, pW, pH)
        val pLeft2 = ControlRect(margin + pW + pGap, pY, pW, pH)
        val pRight2 = ControlRect(w - margin - pW, pY, pW, pH)
        val pRight1 = ControlRect(pRight2.x - pGap - pW, pY, pW, pH)
        fun centred(x: Int, y: Int, size: Int) = ControlRect(x - size / 2, y - size / 2, size, size)
        val dpadX = margin + maxOf(px(maxPad), 2 * pW + pGap) / 2
        val dpad = centred(dpadX, axisY, pad)
        val stylus = centred(stickX, stickY, stick)
        val a = centred(faceX, axisY - delta, diameter)
        val b = centred(faceX + delta, axisY, diameter)
        val c = centred(faceX, axisY + delta, diameter)
        val action = centred(faceX - delta, axisY, diameter)
        // Stable ID order: A, P2, Action, P1, P4, C, P3, B.
        val actions = listOf(a, pLeft2, action, pLeft1, pRight2, c, pRight1, b)
        val toolH = px(28f); val toolGap = px(6f); val hostGap = px(18f)
        // Never let a toolbar button cover a raised P key on narrow windows.
        val toolRoom = if (portrait) w else pRight1.x - (pLeft2.x + pW) - 2 * px(8f)
        val toolW = minOf(px(60f), (toolRoom - 4 * toolGap - hostGap) / 6).coerceAtLeast(1)
        val toolStart = (w - 6 * toolW - 4 * toolGap - hostGap) / 2
        val tools = (0..5).map { i ->
            ControlRect(toolStart + i * (toolW + toolGap) + if (i >= 3) hostGap - toolGap else 0,
                px(14f), toolW, toolH)
        }
        val top = px(14f) + toolH + px(16f)
        val screen = if (portrait) {
            ControlRect(margin, top, (w - 2 * margin).coerceAtLeast(1), (pY - px(16f) - top).coerceAtLeast(1))
        } else {
            val left = maxOf(pLeft2.x + pW, dpad.x + dpad.w) + px(gutter)
            val right = minOf(pRight1.x, action.x, stylus.x) - px(gutter)
            ControlRect(left, top, (right - left).coerceAtLeast(1), (h - top - px(16f)).coerceAtLeast(1))
        }
        return PocketPcGeometry(screen, actions, tools, dpad, stylus)
    }
}
