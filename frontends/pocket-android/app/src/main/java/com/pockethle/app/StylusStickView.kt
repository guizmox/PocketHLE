package com.pockethle.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.view.MotionEvent
import android.view.View
import kotlin.math.hypot

/** Independent pointer capture: another finger can keep movement/fire held. */
internal class StylusStickView(context: Context) : View(context) {
    var onAim: ((Float, Float) -> Unit)? = null
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var pointer = -1
    private var xAxis = 0f
    private var yAxis = 0f
    init { contentDescription = "Pocket PC stylus / look stick"; isClickable = true }
    fun clearInput() { pointer = -1; xAxis = 0f; yAxis = 0f; onAim?.invoke(0f, 0f); invalidate() }
    override fun onDraw(canvas: Canvas) {
        val r = minOf(width, height) * .43f
        paint.color = 0xb0303a47.toInt(); canvas.drawCircle(width/2f, height/2f, r, paint)
        paint.color = 0xff419dcc.toInt(); canvas.drawCircle(width/2f+xAxis*r*.65f, height/2f+yAxis*r*.65f, r*.35f, paint)
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        when(event.actionMasked) {
            MotionEvent.ACTION_DOWN -> { pointer=event.getPointerId(0); parent.requestDisallowInterceptTouchEvent(true) }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> { clearInput(); parent.requestDisallowInterceptTouchEvent(false); return true }
            MotionEvent.ACTION_POINTER_UP -> if(event.getPointerId(event.actionIndex)==pointer) { clearInput(); return true }
        }
        val i=event.findPointerIndex(pointer)
        if(i>=0) {
            val r=(minOf(width,height)*.4f).coerceAtLeast(1f)
            var x=(event.getX(i)-width/2f)/r; var y=(event.getY(i)-height/2f)/r
            val n=hypot(x,y); if(n>1f) { x/=n; y/=n }
            if(n<.12f) { x=0f; y=0f }
            xAxis=x; yAxis=y; onAim?.invoke(x,y); invalidate()
        }
        return true
    }
}
