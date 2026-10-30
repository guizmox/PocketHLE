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
        layoutDirection = View.LAYOUT_DIRECTION_LTR
        isMotionEventSplittingEnabled = true
        fun button(id: Int, label: String): View = Button(context).apply {
            this.id = id; text = label; contentDescription = label
            setTextColor(Color.WHITE); textSize = 18f
            minWidth = 0; minHeight = 0; minimumWidth = 0; minimumHeight = 0
            setPadding(0,0,0,0); background = controlStyle.backgroundShape(); backgroundTintList = null; stateListAnimator = null
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
        button(R.id.btn_fullscreen, "Full").apply { (this as Button).textSize=13f; background=controlStyle.backgroundShape(emulatorAction=true) }
        button(R.id.btn_stop_emulation, "Exit").apply {
            (this as Button).textSize = 13f
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

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec); val h = MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w,h)
        geometry = GizmondoGeometry.calculate(w, h, dp, bigDirectionalControls)
        val padSize = geometry.dpad.w
        dpad.measure(MeasureSpec.makeMeasureSpec(padSize, MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(padSize, MeasureSpec.EXACTLY))
        keys.forEach { (id,v) ->
            val shoulder = id == R.id.btn_soft1 || id == R.id.btn_soft2
            val toolbarScale = geometry.toolbarScale
            val function = id in listOf(R.id.btn_piano1, R.id.btn_piano2, R.id.btn_piano3,
                R.id.btn_piano4, R.id.btn_piano5, R.id.btn_stop_emulation, R.id.btn_fullscreen)
            val bw = if (shoulder) 124f else if (id == R.id.btn_stop_emulation || id == R.id.btn_fullscreen) 64f else 44f
            val bh = if (shoulder) 132f else 44f
            val scale = if (function) toolbarScale else 1f
            v.measure(MeasureSpec.makeMeasureSpec((bw*dp*scale).toInt(),MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec((bh*dp*scale).toInt(),MeasureSpec.EXACTLY))
        }
    }
    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        fun place(id: Int, x: Float, y: Float) { val v = keys.getValue(id); val left=(x-v.measuredWidth/2).toInt(); val top=(y-v.measuredHeight/2).toInt(); v.layout(left,top,left+v.measuredWidth,top+v.measuredHeight) }
        // Physical sides stay fixed in both landscape orientations and every locale.
        val dpadCenter = geometry.dpad.x + geometry.dpad.w / 2f; val actionPadCenter = width-74*dp
        // Bottom edge padding is shared by the 140dp pad and the action diamond.
        val middle = height - 78 * dp
        place(R.id.btn_soft1, dpadCenter, 74 * dp)
        place(R.id.btn_soft2, actionPadCenter, 74 * dp)
        listOf(R.id.btn_a,R.id.btn_c,R.id.btn_b,R.id.btn_action).forEachIndexed { i,id -> val offsets=listOf(0f to -46f,-46f to 0f,46f to 0f,0f to 46f); place(id,actionPadCenter+offsets[i].first*dp,middle+offsets[i].second*dp) }
        val padLeft = (dpadCenter - dpad.measuredWidth / 2f).toInt()
        val padTop = geometry.dpad.y
        dpad.layout(padLeft, padTop, padLeft + dpad.measuredWidth, padTop + dpad.measuredHeight)
        // Centre the complete toolbar, so the hardware group itself shifts left.
        // Exit has a 28dp gap after Power instead of the hardware buttons' 8dp gaps.
        val toolbarScale = geometry.toolbarScale
        val unit = dp * toolbarScale
        val start = geometry.screen.x + (geometry.screen.w - 416 * unit) / 2f
        val topCenter = 8 * dp + 22 * unit
        listOf(R.id.btn_piano1,R.id.btn_piano2,R.id.btn_piano3,R.id.btn_piano4,R.id.btn_piano5)
            .forEachIndexed { i,id -> place(id, start + (22 + i * 52) * unit, topCenter) }
        place(R.id.btn_stop_emulation, start + 312 * unit, topCenter)
        place(R.id.btn_fullscreen, start + 384 * unit, topCenter)
    }
}

/** Symbols traced as native vector primitives from the desktop Gizmondo skin. */
private class FunctionButton(context: Context, private val kind: Int) : AppCompatImageButton(context) {
    private val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply { color=Color.WHITE; strokeWidth=1.8f; strokeCap=Paint.Cap.ROUND; strokeJoin=Paint.Join.ROUND }
    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.save(); canvas.translate(width/2f-12*resources.displayMetrics.density,height/2f-12*resources.displayMetrics.density)
        canvas.scale(resources.displayMetrics.density,resources.displayMetrics.density)
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
