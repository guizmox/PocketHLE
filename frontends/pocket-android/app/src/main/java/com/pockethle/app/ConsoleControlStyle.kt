package com.pockethle.app

import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.StateListDrawable

/** One palette and shape for Gizmondo and Pocket PC; each view owns its drawable. */
internal class ConsoleControlStyle(private val dp: Float) {
    fun backgroundShape(emulatorAction: Boolean = false): StateListDrawable {
        fun shape(top: Int, bottom: Int, outline: Int) = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM, intArrayOf(top, bottom)
        ).apply {
            cornerRadius = 14 * dp
            setStroke(dp.toInt().coerceAtLeast(1), outline)
        }
        // Keep a visible highlight for the entire hold, including simultaneous keys.
        return StateListDrawable().apply {
            addState(intArrayOf(android.R.attr.state_pressed),
                if (emulatorAction) shape(0xffdd8b49.toInt(), 0xffa94d20.toInt(), 0xffffc48f.toInt())
                else shape(0xff3288bf.toInt(), 0xff195b8c.toInt(), 0xff91d6ff.toInt()))
            addState(intArrayOf(),
                if (emulatorAction) shape(0xffa9592d.toInt(), 0xff71371f.toInt(), 0xffcf8b58.toInt())
                else shape(0xff303a47.toInt(), 0xff141c28.toInt(), 0xff506078.toInt()))
        }
    }
}
