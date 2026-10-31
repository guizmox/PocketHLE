package com.pockethle.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.util.AttributeSet
import android.view.View
import android.widget.Button
import android.widget.FrameLayout
import androidx.appcompat.widget.AppCompatImageButton

/** Fifteen SDK controls. Independent child views retain Android multitouch dispatch. */
class GizmondoControls(context: Context, attrs: AttributeSet? = null) : FrameLayout(context, attrs) {
    private val dp = resources.displayMetrics.density
    private val controlStyle = ConsoleControlStyle(dp)
    private val keys = mutableMapOf<Int, View>()
    val dpad = TouchDpad(context)
    init {
        dpad.translucentStyle = true
        layoutDirection = View.LAYOUT_DIRECTION_LTR
        isMotionEventSplittingEnabled = true
        fun button(id: Int, label: String): Button = Button(context).apply {
            this.id = id; text = label; contentDescription = label
            setTextColor(0xb8ffffff.toInt()); textSize = 18f
            minWidth = 0; minHeight = 0; minimumWidth = 0; minimumHeight = 0
            setPadding(0,0,0,0); background = controlStyle.backgroundShape(oval = id in listOf(R.id.btn_a, R.id.btn_b, R.id.btn_c, R.id.btn_action), translucent = true); backgroundTintList = null; stateListAnimator = null
            keys[id] = this; addView(this)
        }
        button(R.id.btn_soft1, "L"); button(R.id.btn_soft2, "R")
        button(R.id.btn_a, "■").contentDescription = "Stop"
        button(R.id.btn_c, "◀◀").contentDescription = "Rewind"
        button(R.id.btn_b, "▶▶").contentDescription = "Forward"
        button(R.id.btn_action, "▶").contentDescription = "Play"
        addView(dpad)
        val icons = listOf(R.id.btn_piano1, R.id.btn_piano2, R.id.btn_piano3, R.id.btn_piano4, R.id.btn_piano5)
        val names = listOf("Home", "Volume", "Brightness", "Geofence", "Power")
        icons.forEachIndexed { index, id ->
            val icon = FunctionButton(context, index).apply {
                this.id = id; contentDescription = names[index]; background = controlStyle.backgroundShape(); backgroundTintList = null
            }
            keys[id] = icon; addView(icon)
        }
        button(R.id.btn_fullscreen, "Full").apply { textSize=13f; setTextColor(Color.WHITE); background=controlStyle.backgroundShape(emulatorAction=true) }
        button(R.id.btn_stop_emulation, "Exit").apply {
            textSize = 13f; setTextColor(Color.WHITE)
            background = controlStyle.backgroundShape(emulatorAction = true)
        }
    }
    var bigDirectionalControls: Boolean = false
        set(value) {
            if (field == value) return
            clearPressedControls(); field = value
            requestLayout()
        }
    private var geometry = GizmondoGeometry.calculate(1, 1, dp)
    fun clearPressedControls() {
        keys.values.forEach { it.isPressed = false }
        dpad.clearInput()
    }

    var maximizeScreen: Boolean = false
        set(value) {
            if (field == value) return
            field = value; requestLayout(); parent?.requestLayout()
        }
    private fun targets(): Map<Int, ControlRect> = (
        listOf(R.id.btn_soft1, R.id.btn_soft2).zip(geometry.shoulders) +
        listOf(R.id.btn_a, R.id.btn_c, R.id.btn_b, R.id.btn_action).zip(geometry.actions) +
        listOf(R.id.btn_piano1, R.id.btn_piano2, R.id.btn_piano3, R.id.btn_piano4,
            R.id.btn_piano5, R.id.btn_stop_emulation, R.id.btn_fullscreen).zip(geometry.toolbar)
    ).toMap()

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec); val h = MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w, h)
        geometry = GizmondoGeometry.calculate(w, h, dp, bigDirectionalControls, maximizeScreen)
        fun measure(view: View, rect: ControlRect) {
            view.measure(MeasureSpec.makeMeasureSpec(rect.w, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(rect.h, MeasureSpec.EXACTLY))
        }
        measure(dpad, geometry.dpad)
        targets().forEach { (id, rect) ->
            val view = keys.getValue(id)
            if (view is Button) view.textSize = when (id) {
                R.id.btn_stop_emulation, R.id.btn_fullscreen -> 13f * geometry.toolbarScale
                R.id.btn_soft1, R.id.btn_soft2 -> 18f
                else -> 18f * geometry.controlScale
            }
            measure(view, rect)
        }
    }
    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        fun place(view: View, rect: ControlRect) = view.layout(rect.x, rect.y, rect.x + rect.w, rect.y + rect.h)
        place(dpad, geometry.dpad)
        targets().forEach { (id, rect) -> place(keys.getValue(id), rect) }
    }

}

/** Symbols traced as native vector primitives from the desktop Gizmondo skin. */
private class FunctionButton(context: Context, private val kind: Int) : AppCompatImageButton(context) {
    private val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply { color=Color.WHITE; strokeWidth=1.8f; strokeCap=Paint.Cap.ROUND; strokeJoin=Paint.Join.ROUND }
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val scale = minOf(resources.displayMetrics.density, width / 28f, height / 28f)
        canvas.save(); canvas.translate(width / 2f - 12 * scale, height / 2f - 12 * scale)
        canvas.scale(scale, scale)
        ink.style=Paint.Style.STROKE
        when(kind) {
            0 -> { ink.style=Paint.Style.FILL; canvas.drawPath(Path().apply { moveTo(2f,11f);lineTo(5f,8f);lineTo(5f,3f);lineTo(8f,3f);lineTo(8f,5f);lineTo(12f,1f);lineTo(23f,11f);lineTo(20f,11f);lineTo(20f,23f);lineTo(14f,23f);lineTo(14f,15f);lineTo(10f,15f);lineTo(10f,23f);lineTo(4f,23f);lineTo(4f,11f);close() },ink) }
            1 -> { ink.style=Paint.Style.FILL; canvas.drawPath(Path().apply { moveTo(2f,8f);lineTo(6f,8f);lineTo(12f,3f);lineTo(12f,21f);lineTo(6f,16f);lineTo(2f,16f);close() },ink); ink.style=Paint.Style.STROKE; canvas.drawArc(RectF(9f,6f,21f,18f),-65f,130f,false,ink);canvas.drawArc(RectF(6f,2f,26f,22f),-60f,120f,false,ink) }
            2 -> { canvas.drawCircle(12f,12f,5f,ink); for(i in 0..7) { val a=i*Math.PI/4;canvas.drawLine(12f+8f*kotlin.math.cos(a).toFloat(),12f+8f*kotlin.math.sin(a).toFloat(),12f+11f*kotlin.math.cos(a).toFloat(),12f+11f*kotlin.math.sin(a).toFloat(),ink) } }
            3 -> { canvas.drawLine(12f,10f,12f,22f,ink);canvas.drawLine(12f,13f,6f,22f,ink);canvas.drawLine(12f,13f,18f,22f,ink); canvas.drawCircle(12f,8f,1f,ink);canvas.drawArc(RectF(5f,1f,19f,15f),-150f,120f,false,ink);canvas.drawArc(RectF(1f,-3f,23f,19f),-150f,120f,false,ink);canvas.drawArc(RectF(5f,1f,19f,15f),30f,120f,false,ink);canvas.drawArc(RectF(1f,-3f,23f,19f),30f,120f,false,ink) }
            4 -> { canvas.drawArc(RectF(3f,3f,21f,21f),-50f,280f,false,ink);canvas.drawLine(12f,1f,12f,11f,ink) }
        }
        canvas.restore()
    }
}
