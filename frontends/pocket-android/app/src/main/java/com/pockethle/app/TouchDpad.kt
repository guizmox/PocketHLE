package com.pockethle.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.view.MotionEvent
import android.view.View

/** One continuous touch target: slide without lifting; diagonals hold two keys. */
class TouchDpad(context: Context) : View(context) {
    var onDirectionsChanged: ((View, Int) -> Unit)? = null
    private var pointerId = -1
    private var directions = 0
    private val ink = Paint(Paint.ANTI_ALIAS_FLAG)
    private val cross = Path()
    private val arm = RectF()

    init { contentDescription = "Directional pad"; isClickable = true }

    private fun update(mask: Int) {
        if (mask == directions) return
        directions = mask
        invalidate()
        onDirectionsChanged?.invoke(this, mask)
    }

    fun clearInput() {
        pointerId = -1
        update(0)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                pointerId = event.getPointerId(event.actionIndex)
                parent?.requestDisallowInterceptTouchEvent(true)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                clearInput()
                parent?.requestDisallowInterceptTouchEvent(false)
                if (event.actionMasked == MotionEvent.ACTION_UP) performClick()
                return true
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (event.getPointerId(event.actionIndex) == pointerId) clearInput()
                return true
            }
        }
        // A second finger never steals the pad from the original thumb.
        val index = event.findPointerIndex(pointerId)
        if (index >= 0) update(DpadDirections.at(event.getX(index), event.getY(index),
            width.toFloat(), height.toFloat()))
        return true
    }

    override fun performClick(): Boolean { super.performClick(); return true }

    override fun onDetachedFromWindow() { clearInput(); super.onDetachedFromWindow() }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save()
        canvas.scale(width / 140f, height / 140f)
        ink.style = Paint.Style.FILL
        ink.color = 0xff101824.toInt()
        canvas.drawCircle(70f, 70f, 69f, ink)
        cross.reset()
        cross.moveTo(48f, 6f); cross.lineTo(92f, 6f); cross.lineTo(92f, 48f)
        cross.lineTo(134f, 48f); cross.lineTo(134f, 92f); cross.lineTo(92f, 92f)
        cross.lineTo(92f, 134f); cross.lineTo(48f, 134f); cross.lineTo(48f, 92f)
        cross.lineTo(6f, 92f); cross.lineTo(6f, 48f); cross.lineTo(48f, 48f); cross.close()
        ink.color = 0xff303a47.toInt(); canvas.drawPath(cross, ink)
        ink.style = Paint.Style.STROKE; ink.strokeWidth = 2f
        ink.color = 0xff64768e.toInt(); canvas.drawPath(cross, ink)
        ink.style = Paint.Style.FILL
        // The same arm is rotated for each of the four physical contacts.
        val bits = intArrayOf(DpadDirections.UP, DpadDirections.RIGHT, DpadDirections.DOWN, DpadDirections.LEFT)
        bits.forEachIndexed { i, bit ->
            canvas.save(); canvas.rotate(i * 90f, 70f, 70f)
            if (directions and bit != 0) {
                ink.color = 0xff3288bf.toInt(); arm.set(50f, 8f, 90f, 49f)
                canvas.drawRoundRect(arm, 5f, 5f, ink)
            }
            ink.color = Color.WHITE
            cross.reset(); cross.moveTo(70f, 19f); cross.lineTo(61f, 31f)
            cross.lineTo(79f, 31f); cross.close(); canvas.drawPath(cross, ink)
            canvas.restore()
        }
        ink.color = 0xff172230.toInt(); canvas.drawCircle(70f, 70f, 11f, ink)
        canvas.restore()
    }
}
