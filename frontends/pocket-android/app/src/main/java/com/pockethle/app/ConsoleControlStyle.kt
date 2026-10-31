package com.pockethle.app

import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable

/** One palette and shape for Gizmondo and Pocket PC; each view owns its drawable. */
internal class ConsoleControlStyle(private val dp: Float) {
    fun backgroundShape(emulatorAction: Boolean = false, oval: Boolean = false, translucent: Boolean = false): StateListDrawable {
        fun shape(top: Int, bottom: Int, outline: Int) = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(top, bottom)
        ).apply {
            if (oval) setShape(GradientDrawable.OVAL) else setCornerRadius(14 * dp)
            setStroke(dp.toInt().coerceAtLeast(1), outline)
        }
        // Keep a visible highlight for the entire hold, including simultaneous keys.
        return StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed),
                if (translucent) shape(0x993288bf.toInt(), 0x99195b8c.toInt(), 0xc5ffffff.toInt())
                else if (emulatorAction) shape(0xffdd8b49.toInt(), 0xffa94d20.toInt(), 0xffffc48f.toInt())
                else shape(0xff3288bf.toInt(), 0xff195b8c.toInt(), 0xff91d6ff.toInt()))
            addState(intArrayOf(),
                if (translucent) shape(0x4510151d, 0x4510151d, 0x70ffffff)
                else if (emulatorAction) shape(0xffa9592d.toInt(), 0xff71371f.toInt(), 0xffcf8b58.toInt())
                else shape(0xff303a47.toInt(), 0xff141c28.toInt(), 0xff506078.toInt()))
        }
    }
    /** No resting fill: the game remains visible behind the toolbar. */
    fun toolbarBackground(emulatorAction: Boolean = false): StateListDrawable {
        val rgb = if (emulatorAction) 0x00cf8b58 else 0x00ffffff
        fun outline(pressed: Boolean) = GradientDrawable().apply {
            setCornerRadius(18f * dp)
            setColor(if (pressed) 0x24000000 or rgb else android.graphics.Color.TRANSPARENT)
            setStroke(dp.toInt().coerceAtLeast(1), (if (pressed) 0xff000000.toInt() else 0x70000000) or rgb)
        }
        return StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed), outline(true))
            addState(intArrayOf(), outline(false))
        }
    }
    fun toolbarTextColors(emulatorAction: Boolean = false) = android.content.res.ColorStateList(
        arrayOf(intArrayOf(-android.R.attr.state_enabled), intArrayOf(android.R.attr.state_pressed), intArrayOf()),
        intArrayOf(
            0x40000000 or (if (emulatorAction) 0x00cf8b58 else 0x00ffffff),
            0xff000000.toInt() or (if (emulatorAction) 0x00cf8b58 else 0x00ffffff),
            (if (emulatorAction) 0xd0000000.toInt() else 0xb8000000.toInt()) or
                (if (emulatorAction) 0x00cf8b58 else 0x00ffffff)
        )
    )

}
