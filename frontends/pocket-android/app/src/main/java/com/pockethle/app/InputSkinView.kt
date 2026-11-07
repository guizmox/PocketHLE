package com.pockethle.app

import android.content.Context
import android.graphics.*
import android.view.MotionEvent
import android.view.View
import kotlin.math.hypot
import org.json.JSONArray

internal data class InputSkinControl(
    val input: String?,
    val label: String,
    val rect: RectF,
    val liveKey: String? = input,
)

/** Uses the exact PNGs and geometry bundled by the desktop editor. */
internal class InputSkinView(
    context: Context,
    val pad: Boolean,
    val state: InputEditorState,
    var gizmondo: Boolean,
    var selected: String?,
    val live: InputLiveState,
    val select: (String) -> Unit,
) : View(context) {
    private val bitmap =
        skins.getOrPut(pad) {
            context.assets.open(if (pad) "controller-flat.png" else "keyboard-flat.png").use {
                BitmapFactory.decodeStream(it)
            }
        }
    private val logicalWidth = if (pad) 760f else 1000f
    private val logicalHeight = if (pad) 500f else 340f
    val controls: List<InputSkinControl> = loadControls(context, pad)
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
    private var downX = 0f
    private var downY = 0f

    init {
        contentDescription = if (pad) "Generic controller bindings" else "AZERTY keyboard bindings"
        isClickable = true
    }

    override fun onMeasure(w: Int, h: Int) {
        val width = MeasureSpec.getSize(w)
        setMeasuredDimension(width, (width * logicalHeight / logicalWidth).toInt())
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val scale = width / logicalWidth
        canvas.save()
        canvas.scale(scale, scale)
        paint.color = Color.WHITE
        paint.style = Paint.Style.FILL
        canvas.drawBitmap(bitmap, null, RectF(0f, 0f, logicalWidth, logicalHeight), paint)
        for (control in controls) {
            val input = control.input
            val target = input?.let { state.assignment(it, pad, gizmondo) }
            val color =
                if (pad && target == "host" && DeckInputPolicy.tap(input!!) == null) LONG_PRESS
                else color(target)
            val rect = control.rect
            val pressed = control.liveKey?.let { live.isDown(it, pad) } ?: false
            if (pressed) {
                paint.style = Paint.Style.FILL
                paint.color = Color.rgb(30, 62, 72)
                if (pad && input != null && DeckInputPolicy.reserved(input, gizmondo))
                    canvas.drawRoundRect(
                        RectF(
                            rect.centerX() - 23f,
                            rect.centerY() - 12f,
                            rect.centerX() + 23f,
                            rect.centerY() + 12f,
                        ),
                        10f,
                        10f,
                        paint,
                    )
                else if (pad) canvas.drawCircle(rect.centerX(), rect.centerY(), 22f, paint)
                else canvas.drawRoundRect(rect, 4f, 4f, paint)
                paint.style = Paint.Style.STROKE
                paint.color = LIVE
                paint.strokeWidth = 3f
                if (pad && input != null && DeckInputPolicy.reserved(input, gizmondo))
                    canvas.drawRoundRect(
                        RectF(
                            rect.centerX() - 23f,
                            rect.centerY() - 12f,
                            rect.centerX() + 23f,
                            rect.centerY() + 12f,
                        ),
                        10f,
                        10f,
                        paint,
                    )
                else if (pad) canvas.drawCircle(rect.centerX(), rect.centerY(), 22f, paint)
                else canvas.drawRoundRect(rect, 4f, 4f, paint)
            }
            if (pressed && pad && input == "Misc1") {
                paint.style = Paint.Style.FILL
                paint.color = Color.LTGRAY
                for (dx in listOf(-7f, 0f, 7f)) canvas.drawCircle(
                    rect.centerX() + dx,
                    rect.centerY(),
                    1.8f,
                    paint,
                )
            }
            if (!pad) {
                if (input != null) {
                    paint.style = Paint.Style.STROKE
                    paint.strokeWidth =
                        if (DeckInputPolicy.reservedKey(input)) 2.5f
                        else if (input == selected) 2.5f else 1f
                    paint.color =
                        if (DeckInputPolicy.reservedKey(input)) RESERVED
                        else if (pressed) LIVE else if (input == selected) BLUE else color
                    canvas.drawRoundRect(rect, 4f, 4f, paint)
                }
                paint.style = Paint.Style.FILL
                paint.color = if (input == null) Color.LTGRAY else Color.WHITE
                paint.textAlign = Paint.Align.CENTER
                paint.textSize = 13f
                canvas.drawText(
                    control.label,
                    rect.centerX(),
                    if (input == null) rect.centerY() + 5 else rect.top + rect.height() * .37f,
                    paint,
                )
                if (input != null) {
                    if (state.longTarget(input, false, gizmondo) != null) {
                        paint.color = LONG_PRESS
                        canvas.drawCircle(rect.right - 5f, rect.top + 5f, 2.5f, paint)
                    }
                    paint.color = color
                    paint.textSize = 10f
                    canvas.drawText(
                        state.shortFor(input, pad, gizmondo),
                        rect.centerX(),
                        rect.top + rect.height() * .84f,
                        paint,
                    )
                }
            } else if (input != null) {
                var x = rect.centerX()
                var y = rect.centerY()
                if (input == "Misc1") y += 24f
                if (input.startsWith("LeftStick") || input.startsWith("RightStick")) {
                    if (input.endsWith("Left")) x -= 14f
                    if (input.endsWith("Right")) x += 14f
                    if (input.endsWith("Up")) y -= 14f
                    if (input.endsWith("Down")) y += 14f
                }
                val label =
                    when (target) {
                        "center_tap" -> "Tap"
                        "rotate_screen" -> "Rotate"
                        "stylus:0" -> "PenU"
                        "stylus:1" -> "PenD"
                        "stylus:2" -> "PenL"
                        "stylus:3" -> "PenR"
                        "action" -> if (gizmondo) "Play" else "Act"
                        "soft1" -> if (gizmondo) "L1" else "Tab"
                        "soft2" -> if (gizmondo) "R1" else "Esc"
                        "pocket_pc_app1" -> "P1"
                        "pocket_pc_app2" -> "P2"
                        "pocket_pc_app3" -> "P3"
                        "pocket_pc_app4" -> "P4"
                        else -> state.shortFor(input, true, gizmondo)
                    }
                paint.style = Paint.Style.FILL
                paint.color = color
                paint.textSize = 14f
                paint.textAlign = Paint.Align.CENTER
                canvas.drawText(label, x, y + 5f, paint)
                val customHold = state.longTarget(input, true, gizmondo)
                val holdLabel =
                    customHold?.let { state.short(it, gizmondo) }
                        ?: if (target == "host")
                            DeckInputPolicy.hold(input, gizmondo)?.let { DeckInputPolicy.short(it) }
                        else null
                if (holdLabel != null) {
                    paint.color = LONG_PRESS
                    paint.textSize = 13f
                    canvas.drawText(
                        "Hold: $holdLabel",
                        rect.centerX(),
                        rect.centerY() + if (input == "Misc1") 43f else 26f,
                        paint,
                    )
                }
                if (input == selected && !DeckInputPolicy.reserved(input, gizmondo)) {
                    paint.style = Paint.Style.STROKE
                    paint.strokeWidth = 3f
                    paint.color = BLUE
                    canvas.drawCircle(rect.centerX(), rect.centerY(), 18f, paint)
                }
            }
            if (pad && input != null && DeckInputPolicy.reserved(input, gizmondo)) {
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = 2.5f
                paint.color = RESERVED
                canvas.drawRoundRect(
                    RectF(
                        rect.centerX() - 23f,
                        rect.centerY() - 12f,
                        rect.centerX() + 23f,
                        rect.centerY() + 12f,
                    ),
                    10f,
                    10f,
                    paint,
                )
            }
        }
        paint.style = Paint.Style.FILL
        canvas.restore()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                downX = event.x
                downY = event.y
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (
                    hypot(event.x - downX, event.y - downY) < 8 * resources.displayMetrics.density
                ) {
                    val x = event.x * logicalWidth / width
                    val y = event.y * logicalWidth / width
                    controls
                        .filter { it.input != null && it.rect.contains(x, y) }
                        .minByOrNull { hypot(x - it.rect.centerX(), y - it.rect.centerY()) }
                        ?.input
                        ?.let { select(it) }
                    performClick()
                }
                return true
            }
            MotionEvent.ACTION_CANCEL -> return true
        }
        return true
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }

    companion object {
        private val skins = mutableMapOf<Boolean, Bitmap>()
        val RESERVED = Color.rgb(225, 95, 95)
        val LIVE = Color.rgb(115, 235, 250)
        val LONG_PRESS = Color.rgb(231, 181, 96)
        val PURPLE = Color.rgb(176, 135, 231)
        val GREEN = Color.rgb(93, 194, 162)
        val BLUE = Color.rgb(93, 179, 235)

        fun color(target: String?) =
            when {
                target == null -> Color.rgb(145, 149, 156)
                target == "host" || target == "rotate_screen" -> Color.rgb(176, 135, 231)
                target == "center_tap" || target.startsWith("stylus:") -> Color.rgb(93, 194, 162)
                else -> BLUE
            }

        fun loadControls(context: Context, pad: Boolean): List<InputSkinControl> {
            val data =
                JSONArray(
                    context.assets
                        .open(if (pad) "controller-layout.json" else "keyboard-layout.json")
                        .bufferedReader()
                        .use { it.readText() }
                )
            return (0 until data.length()).map { index ->
                val item = data.getJSONObject(index)
                val name =
                    if (item.isNull("input")) null
                    else InputBindings.canonical(item.getString("input"))
                val rect =
                    if (pad) {
                        val x = item.getDouble("x").toFloat()
                        val y = item.getDouble("y").toFloat()
                        RectF(x - 18, y - 18, x + 18, y + 18)
                    } else
                        item.getJSONArray("rect").let { r ->
                            val x = r.getDouble(0).toFloat()
                            val y = r.getDouble(1).toFloat()
                            RectF(x, y, x + r.getDouble(2).toFloat(), y + r.getDouble(3).toFloat())
                        }
                InputSkinControl(
                    name,
                    item.getString("label"),
                    rect,
                    item.optString("live_key").takeIf { it.isNotEmpty() } ?: name,
                )
            }
        }
    }
}
