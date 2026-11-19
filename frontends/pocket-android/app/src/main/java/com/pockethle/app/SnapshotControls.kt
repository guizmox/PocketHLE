package com.pockethle.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout

/** Two small native UI controls, at the physical window corners, independent of the guest image. */
internal class SnapshotControls(
    context: Context,
    private val host: FrameLayout,
    action: (restore: Boolean) -> Unit,
) {
    private val radius = SnapshotControlGeometry.radius(context.resources.displayMetrics.density)
    private val save = Corner(context, false, "S").apply {
        contentDescription = "Save snapshot"
        setOnClickListener { action(false) }
    }
    private val restore = Corner(context, true, "L").apply {
        contentDescription = "Restore snapshot"
        setOnClickListener { action(true) }
    }
    init {
        for (view in listOf(save, restore)) {
            view.visibility = View.GONE
            view.isFocusable = true
            host.addView(view, FrameLayout.LayoutParams(radius, radius))
        }
    }
    fun busy(value: Boolean) {
        for (view in listOf(save, restore)) {
            view.isEnabled = !value
            view.alpha = if (value) 0.4f else 1f
        }
    }
    fun place(rect: Rect?) {
        for ((view, right) in listOf(save to false, restore to true)) {
            view.visibility = if (rect == null || rect.isEmpty) View.GONE else View.VISIBLE
            if (rect == null || rect.isEmpty) continue
            val size = minOf(radius, rect.width() / 3, rect.height() / 3).coerceAtLeast(1)
            val left = if (right) rect.right - size else rect.left
            val layout = view.layoutParams as FrameLayout.LayoutParams
            if (layout.width != size || layout.height != size || layout.leftMargin != left || layout.topMargin != rect.top) {
                layout.width = size; layout.height = size
                layout.leftMargin = left; layout.topMargin = rect.top
                layout.gravity = Gravity.TOP or Gravity.LEFT
                view.layoutParams = layout
            }
        }
    }
    fun loadBoundsInWindow(): Rect? {
        if (restore.visibility != View.VISIBLE) return null
        val origin = IntArray(2).also { host.getLocationInWindow(it) }
        val layout = restore.layoutParams as FrameLayout.LayoutParams
        return Rect(origin[0] + layout.leftMargin, origin[1] + layout.topMargin,
            origin[0] + layout.leftMargin + layout.width, origin[1] + layout.topMargin + layout.height)
    }
    private class Corner(context: Context, private val right: Boolean, private val label: String) : View(context) {
        private val dp = resources.displayMetrics.density
        private val style = ConsoleControlStyle(dp)
        private val textColors = style.toolbarTextColors(emulatorAction = true)
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        override fun drawableStateChanged() { super.drawableStateChanged(); invalidate() }
        override fun onDraw(canvas: Canvas) {
            val stroke = dp.coerceAtLeast(1f)
            val pad = stroke / 2f
            val radius = minOf(width, height).toFloat() - stroke
            val center = if (right) width - pad else pad
            val bounds = RectF(center - radius, pad - radius, center + radius, pad + radius)
            val (fill, outline) = style.toolbarColors(isPressed || hasFocus(), emulatorAction = true)
            paint.style = Paint.Style.FILL; paint.color = fill
            canvas.drawArc(bounds, if (right) 90f else 0f, 90f, true, paint)
            paint.style = Paint.Style.STROKE; paint.strokeWidth = stroke; paint.color = outline
            canvas.drawArc(bounds, if (right) 90f else 0f, 90f, true, paint)
            paint.style = Paint.Style.FILL
            paint.color = textColors.getColorForState(drawableState, textColors.defaultColor)
            paint.textSize = minOf(13f * dp, radius * 0.45f)
            paint.textAlign = Paint.Align.CENTER
            val metrics = paint.fontMetrics
            val x = center + (if (right) -1 else 1) * radius * 0.42f
            val y = pad + radius * 0.42f - (metrics.ascent + metrics.descent) / 2f
            canvas.drawText(label, x, y, paint)
        }
        override fun onTouchEvent(event: MotionEvent): Boolean {
            if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                val dx = if (right) width - event.x else event.x
                if (dx * dx + event.y * event.y > width.toFloat() * width) return false
            }
            return super.onTouchEvent(event)
        }
        override fun performClick(): Boolean = super.performClick()
    }
}
