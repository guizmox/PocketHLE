package com.pockethle.app

import kotlin.math.roundToInt

/** Physical-size control groups anchored near the thumbs, never stretched with
 * tablet height. Only host geometry changes; all existing input IDs are reused. */
internal object PocketPcControllerGeometry {
    fun calculate(width: Int, height: Int, density: Float, big: Boolean): PocketPcGeometry {
        val w = width.coerceAtLeast(1); val h = height.coerceAtLeast(1)
        val portrait = w <= h
        val pWidth = if (portrait) 76f else 94f
        val normalPad = 156f
        // Preserve the previous density fit envelope for P/face key sizes.
        // The actual stick below is now sized from the D-pad and free space.
        val normalStick = if (portrait) 104f else 100f
        val maxPad = normalPad * 4f / 3f
        val maxStick = normalStick * 4f / 3f
        val diamondDiameter = 64f
        // Oval action buttons reject DOWN events outside their visible circle.
        // Their bounding rectangles can overlap at empty corners safely.
        val diamondOffset = 50f
        val stickDrop = if (portrait) 124f else 140f
        // Historical fit-envelope clearance, retained to keep key dimensions.
        val stickSideGap = 50f
        val edge = if (portrait) 16f else 8f
        val gutter = if (portrait) 16f else 6f
        val groupWidth = pWidth * 2f + 12f
        val leftWidth = maxOf(groupWidth, maxPad)
        // Measure the actual right-bank envelope. The stick centre is already
        // offset from the face-button centre, so only its radius belongs here.
        val rightWidth = if (portrait) maxOf(2f * diamondOffset + diamondDiameter,
            diamondOffset + diamondDiameter + stickSideGap + maxStick / 2f)
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
        val margin = px(edge); var pW = px(pWidth); val pH = px(76f)
        val pGap = px(12f)
        var pad = px(normalPad * if (big) 4f / 3f else 1f)
        var stick = pad
        var diameter = px(diamondDiameter); var delta = px(diamondOffset)
        val toolH = px(24f)
        val toolY = if (portrait) h - margin - toolH else px(8f)
        val top = toolY + toolH + px(4f)
        if (!portrait && w / density.coerceAtLeast(.1f) < 800f) {
            val normalBank=maxOf(px(normalPad),2*pW+pGap,2*delta+diameter)
            val nextScale=(w-2*(margin+normalBank+px(gutter)))/320+1
            val cap=(w-320*nextScale)/2-margin-px(gutter)
            val minKeyWidth=kotlin.math.ceil(44f*density.coerceAtLeast(.1f)).toInt()
            // Size the P keys and diamond from the Normal envelope in both
            // modes; Big enlarges its directional controls without overlap.
            if (nextScale in 2..4 && h-top-px(16f)>=240*nextScale &&
                cap>=px(normalPad) && cap>=(2*delta+diameter)*.9f &&
                (cap-pGap)/2>=minKeyWidth) {
                pW=minOf(pW,(cap-pGap)/2)
                if (!big) pad=minOf(pad,cap)
                val ratio=minOf(1f,cap.toFloat()/(2*delta+diameter))
                diameter=(diameter*ratio).toInt().coerceAtLeast(1)
                delta=(delta*ratio).toInt().coerceAtLeast(1)
            }
        }
        val faceX = w - margin - delta - diameter / 2
        val bottom = h - px(16f)
        val stickX: Int
        val stickY: Int
        val axisY: Int
        val pY: Int
        val dpadY: Int
        if (portrait) {
            // Stagger the central stick slightly below the side banks. Big
            // needs additional clearance, computed from the actual hit shapes.
            stickX = w / 2
            val dpadX = margin + maxOf(pad, 2 * pW + pGap) / 2
            val gap = px(6f)
            fun clearance(radius: Int, horizontal: Int): Int =
                if (horizontal >= radius) 0 else kotlin.math.ceil(kotlin.math.sqrt(
                    radius.toDouble() * radius - horizontal.toDouble() * horizontal)).toInt()
            val horizontal = (stickX - (dpadX + pad / 2)).coerceAtLeast(0)
            var offset = px(32f)
            if (horizontal < stick / 2 + gap) {
                offset = maxOf(offset, pad / 2 + clearance(stick / 2 + gap, horizontal))
            }
            for ((cx, dy) in listOf(faceX to -delta, faceX + delta to 0,
                faceX to delta, faceX - delta to 0)) {
                val dx = kotlin.math.abs(stickX - cx)
                val radii = stick / 2 + diameter / 2 + gap
                if (dx < radii) offset = maxOf(offset, dy + clearance(radii, dx))
            }
            stickY = toolY - gap - stick / 2
            axisY = stickY - offset
            dpadY = axisY
            pY = axisY - maxOf(pad / 2, delta + diameter / 2) - pGap - pH
        } else {
            // P keys keep their size but move to the top corners; the action
            // diamond has its own raised axis, independent of the centred pad.
            pY = toolY
            axisY = pY + pH + px(12f) + delta + diameter / 2
            val stickRoom = bottom - (axisY + delta + diameter / 2) - px(12f)
            stick = minOf(pad, stickRoom.coerceAtLeast(1))
            stickX = minOf(faceX, w - margin - stick / 2)
            stickY = bottom - stick / 2
            dpadY = (h / 2).coerceIn(pY + pH + px(12f) + pad / 2, bottom - pad / 2)
        }
        val pLeft1 = ControlRect(margin, pY, pW, pH)
        val pLeft2 = ControlRect(margin + pW + pGap, pY, pW, pH)
        val pRight2 = ControlRect(w - margin - pW, pY, pW, pH)
        val pRight1 = ControlRect(pRight2.x - pGap - pW, pY, pW, pH)
        fun centred(x: Int, y: Int, size: Int) = ControlRect(x - size / 2, y - size / 2, size, size)
        val dpadX = margin + maxOf(pad, 2 * pW + pGap) / 2
        val dpad = centred(dpadX, dpadY, pad)
        val stylus = centred(stickX, stickY, stick)
        val a = centred(faceX, axisY - delta, diameter)
        val b = centred(faceX + delta, axisY, diameter)
        val c = centred(faceX, axisY + delta, diameter)
        val action = centred(faceX - delta, axisY, diameter)
        // Stable ID order: A, P2, Action, P1, P4, C, P3, B.
        val actions = listOf(a, pLeft2, action, pLeft1, pRight2, c, pRight1, b)
        val toolGap = px(6f); val hostGap = px(18f)
        // Never let a toolbar button cover a raised P key on narrow windows.
        val toolRoom = if (portrait) w else pRight1.x - (pLeft2.x + pW) - 2 * px(8f)
        val toolW = minOf(px(60f), (toolRoom - 4 * toolGap - hostGap) / 6).coerceAtLeast(1)
        val toolStart = (w - 6 * toolW - 4 * toolGap - hostGap) / 2
        val tools = (0..5).map { i ->
            ControlRect(toolStart + i * (toolW + toolGap) + if (i >= 3) hostGap - toolGap else 0,
                toolY, toolW, toolH)
        }
        val screen = if (portrait) {
            ControlRect(0, 0, w, (pY - px(6f)).coerceAtLeast(1))
        } else {
            val left = maxOf(pLeft2.x + pW, dpad.x + dpad.w) + px(gutter)
            val right = minOf(pRight1.x, action.x, stylus.x) - px(gutter)
            ControlRect(left, top, (right - left).coerceAtLeast(1), (h - top - px(16f)).coerceAtLeast(1))
        }
        return PocketPcGeometry(screen, actions, tools, dpad, stylus)
    }
}
