package com.pockethle.app

import android.content.Context
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import androidx.appcompat.app.AlertDialog

/** Native focus navigation with controller buttons/sticks, independent of guest bindings. */
class AddDataDialog(context: Context, private val hostKey: (KeyEvent) -> Boolean) : AlertDialog(context) {
    var controllerCancel = true
    private var lastDirection = 0
    private var nextMove = 0L

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (hostKey(event)) return true
        if (event.keyCode == KeyEvent.KEYCODE_ESCAPE) {
            if (event.action == KeyEvent.ACTION_UP && controllerCancel) cancel()
            return true
        }
        val source = event.device?.sources ?: event.source
        val pad = source and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
            source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        if (pad) {
            when (InputBindings.controllerName(event.keyCode)) {
                "South" -> return super.dispatchKeyEvent(KeyEvent(event.downTime, event.eventTime, event.action,
                    KeyEvent.KEYCODE_DPAD_CENTER, event.repeatCount))
                "East" -> {
                    if (event.action == KeyEvent.ACTION_UP && controllerCancel) cancel()
                    return true
                }
            }
        }
        return super.dispatchKeyEvent(event)
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK != InputDevice.SOURCE_JOYSTICK) return super.dispatchGenericMotionEvent(event)
        fun axis(hat: Int, stick: Int): Float {
            val value = event.getAxisValue(hat)
            return if (kotlin.math.abs(value) > 0.5f) value else event.getAxisValue(stick)
        }
        val x = axis(MotionEvent.AXIS_HAT_X, MotionEvent.AXIS_X)
        val y = axis(MotionEvent.AXIS_HAT_Y, MotionEvent.AXIS_Y)
        val direction = when {
            x < -0.5f -> KeyEvent.KEYCODE_DPAD_LEFT
            x > 0.5f -> KeyEvent.KEYCODE_DPAD_RIGHT
            y < -0.5f -> KeyEvent.KEYCODE_DPAD_UP
            y > 0.5f -> KeyEvent.KEYCODE_DPAD_DOWN
            else -> 0
        }
        if (direction != 0 && (direction != lastDirection || event.eventTime >= nextMove)) {
            super.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, direction))
            super.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_UP, direction))
            nextMove = event.eventTime + 180
        }
        lastDirection = direction
        return true
    }
}
