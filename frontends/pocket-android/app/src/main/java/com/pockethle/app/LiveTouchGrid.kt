package com.pockethle.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.Spinner
import android.widget.TextView

/** Touch assignment overlay drawn in the same coordinates as the game image. */
internal class LiveTouchGrid(
    context: Context,
    column: LinearLayout,
    area: FrameLayout,
    private val grid: StylusGridConfig,
    private val bindings: InputBindings,
    private val fullscreenBinding: String?,
    private val releaseGameInput: () -> Unit,
    private val save: () -> Boolean,
) {
    var editing = false
        private set
    private var selected = 0
    private var waiting = false
    private var dirty = false
    private var portrait = grid.columns < grid.rows
    private var navigationDirection = 0
    private var nextNavigation = 0L
    private var gameDisplayRect = DisplayRect(0, 0, 0, 0)
    private var sizes = StylusGridConfig.sizes
    private val held = BooleanArray(6)
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val status = TextView(context).apply { gravity = android.view.Gravity.CENTER }
    private val toggle = Button(context)
    private val sizePicker = Spinner(context)
    private val clear = Button(context).apply {
        text = "Clear Grid"
        visibility = View.GONE
    }
    private val overlay = object : View(context) {
        override fun onDraw(canvas: Canvas) {
            if (gameDisplayRect.width <= 0 || gameDisplayRect.height <= 0) return
            if (editing) {
                paint.style = Paint.Style.FILL
                paint.color = 0x99000000.toInt()
                canvas.drawRect(
                    gameDisplayRect.left.toFloat(), gameDisplayRect.top.toFloat(),
                    (gameDisplayRect.left + gameDisplayRect.width).toFloat(),
                    (gameDisplayRect.top + gameDisplayRect.height).toFloat(), paint,
                )
            }
            val width = gameDisplayRect.width.toFloat() / grid.columns
            val height = gameDisplayRect.height.toFloat() / grid.rows
            for (cell in 0 until grid.columns * grid.rows) {
                val x = gameDisplayRect.left + cell % grid.columns * width
                val y = gameDisplayRect.top + cell / grid.columns * height
                paint.color = if (editing && cell == selected) Color.CYAN else 0x99ffffff.toInt()
                paint.style = Paint.Style.STROKE
                paint.strokeWidth = if (editing && cell == selected) 3f else 1f
                canvas.drawRect(x, y, x + width, y + height, paint)
                paint.style = Paint.Style.FILL
                paint.color = Color.WHITE
                paint.textAlign = Paint.Align.CENTER
                paint.textSize = 12f * resources.displayMetrics.scaledDensity
                paint.setShadowLayer(2f, 0f, 1f, Color.BLACK)
                val label = grid.bindings.firstOrNull { it.cell == cell }?.input
                    ?.let { ControllerLabels.grid(it) } ?: (cell + 1).toString()
                canvas.drawText(label, x + width / 2, y + height / 2, paint)
                paint.clearShadowLayer()
            }
        }

        override fun onTouchEvent(event: MotionEvent): Boolean {
            if (!editing) return false
            if (event.actionMasked == MotionEvent.ACTION_UP && gameDisplayRect.width > 0 && gameDisplayRect.height > 0) {
                val x = (event.x - gameDisplayRect.left) / gameDisplayRect.width
                val y = (event.y - gameDisplayRect.top) / gameDisplayRect.height
                if (x >= 0f && x < 1f && y >= 0f && y < 1f) {
                    toggle.clearFocus()
                    clear.clearFocus()
                    selected = (y * grid.rows).toInt() * grid.columns + (x * grid.columns).toInt()
                    waitForInput()
                }
            }
            return true
        }
    }

    init {
        area.addView(overlay, FrameLayout.LayoutParams(-1, -1))
        val controls = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.CENTER
            addView(sizePicker)
            addView(toggle.apply { id = View.generateViewId() })
            addView(clear.apply { id = View.generateViewId() })
            toggle.nextFocusRightId = clear.id
            clear.nextFocusLeftId = toggle.id
        }
        column.addView(controls)
        column.addView(TextView(context).apply {
            gravity = android.view.Gravity.CENTER
            text = "L1 + R1  Grid size  ·  L2 + R2  Edit  ·  L3 + R3  Clear selected"
        })
        column.addView(status)
        toggle.text = "Configure Grid"
        toggle.setOnClickListener { toggleEditing() }
        clear.setOnClickListener {
            grid.bindings.clear()
            dirty = true
            waiting = false
            status.text = "Grid cleared."
            overlay.invalidate()
        }
        refreshSizes()
    }

    fun updateDisplay(rect: DisplayRect, isPortrait: Boolean) {
        if (rect.width <= 0 || rect.height <= 0) return
        if (portrait != isPortrait || (grid.columns < grid.rows) != isPortrait) {
            portrait = isPortrait
            grid.orient(portrait)
            selected = 0
            waiting = false
            refreshSizes()
        }
        if (gameDisplayRect != rect) {
            gameDisplayRect = rect
            overlay.invalidate()
        }
    }

    private fun refreshSizes() {
        sizes = StylusGridConfig.sizes.map { (c, r) -> if (portrait) r to c else c to r }
        sizePicker.onItemSelectedListener = null
        sizePicker.adapter = ArrayAdapter(sizePicker.context, android.R.layout.simple_spinner_dropdown_item,
            sizes.map { (c, r) -> "$c × $r" })
        sizePicker.setSelection(sizes.indexOf(grid.columns to grid.rows).coerceAtLeast(0))
        sizePicker.onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
            override fun onNothingSelected(parent: android.widget.AdapterView<*>?) {}
            override fun onItemSelected(parent: android.widget.AdapterView<*>?, view: View?, position: Int, id: Long) {
                if (position == sizePicker.selectedItemPosition) resize(position)
            }
        }
    }

    private fun resize(index: Int) {
        val (columns, rows) = sizes.getOrNull(index) ?: return
        if (grid.columns == columns && grid.rows == rows) return
        releaseGameInput()
        grid.columns = columns
        grid.rows = rows
        grid.bindings.clear()
        selected = 0
        waiting = false
        status.text = "Grid size changed. Assignments cleared."
        overlay.invalidate()
        dirty = true
        if (!editing) saveChanges()
    }

    private fun saveChanges(): Boolean {
        if (!dirty) return true
        if (!save()) {
            status.text = "Could not save touch grid."
            return false
        }
        dirty = false
        return true
    }

    private fun toggleEditing() {
        if (editing && !saveChanges()) return
        if (!editing) releaseGameInput()
        editing = !editing
        toggle.clearFocus()
        clear.clearFocus()
        clear.visibility = if (editing) View.VISIBLE else View.GONE
        waiting = false
        toggle.text = if (editing) "Resume Game" else "Configure Grid"
        status.text = if (editing) "Select a zone to assign an input." else ""
        overlay.invalidate()
    }

    private fun waitForInput() {
        waiting = true
        status.text = "Press an input to assign zone ${selected + 1}."
        overlay.invalidate()
    }

    private fun assign(input: GridInput) {
        val error = if (input.source == "keyboard" && bindings.stylusKeyboardReserved(input.name)) {
            "This input is reserved."
        } else grid.bind(selected, input, fullscreenBinding)
        status.text = error ?: "Assigned."
        if (error == null) {
            waiting = false
            overlay.invalidate()
            dirty = true
        }
    }

    private fun chord(index: Int, down: Boolean) {
        val wasDown = held[index]
        held[index] = down
        if (!down || wasDown || !held[index xor 1]) return
        when (index) {
            0, 1 -> {
                val next = (sizes.indexOf(grid.columns to grid.rows) + 1) % sizes.size
                resize(next)
                sizePicker.setSelection(next)
            }
            2, 3 -> toggleEditing()
            4, 5 -> if (editing) {
                waiting = false
                grid.bindings.removeAll { it.cell == selected }
                status.text = "Assignment cleared."
                overlay.invalidate()
                dirty = true
            }
        }
    }

    fun virtualInput(input: GridInput, down: Boolean): Boolean {
        if (!editing) return false
        if (waiting && down && input.source == "controller" && input.name.startsWith("Touch:")) {
            assign(input)
        }
        return true
    }

    fun key(event: KeyEvent): Boolean {
        val down = event.action == KeyEvent.ACTION_DOWN
        val controller = event.isFromSource(android.view.InputDevice.SOURCE_GAMEPAD) ||
            event.isFromSource(android.view.InputDevice.SOURCE_JOYSTICK)
        val name = if (controller) gridControllerName(event.keyCode)?.let {
            GridInput("controller", it).canonical().name
        } else gridKeyName(event)
        if (controller) {
            val index = when (name) {
                "LeftTrigger" -> 0
                "RightTrigger" -> 1
                "LeftTrigger2" -> 2
                "RightTrigger2" -> 3
                "LeftThumb" -> 4
                "RightThumb" -> 5
                else -> -1
            }
            if (index >= 0) {
                if (event.repeatCount == 0) chord(index, down)
                return true
            }
        }
        if (!editing) return false
        if (!waiting && (toggle.hasFocus() || clear.hasFocus())) {
            when (event.keyCode) {
                KeyEvent.KEYCODE_DPAD_UP -> {
                    if (down) {
                        toggle.clearFocus()
                        clear.clearFocus()
                        overlay.invalidate()
                    }
                    return true
                }
                KeyEvent.KEYCODE_BUTTON_A -> {
                    if (down && event.repeatCount == 0) {
                        (if (clear.hasFocus()) clear else toggle).performClick()
                    }
                    return true
                }
                KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT,
                KeyEvent.KEYCODE_DPAD_CENTER, KeyEvent.KEYCODE_ENTER -> return false
            }
        }
        if (!down || event.repeatCount != 0) return true
        if (event.keyCode == KeyEvent.KEYCODE_ESCAPE || event.keyCode == KeyEvent.KEYCODE_BACK) {
            if (waiting) {
                waiting = false
                status.text = "Assignment cancelled."
            } else toggleEditing()
        } else if (waiting && name != null) {
            assign(GridInput(if (controller) "controller" else "keyboard", name))
        } else {
            when (name) {
                "South", "Enter" -> waitForInput()
                "East" -> toggleEditing()
                "DPadLeft", "ArrowLeft", "Left" -> moveSelection(-1, 0)
                "DPadRight", "ArrowRight", "Right" -> moveSelection(1, 0)
                "DPadUp", "ArrowUp", "Up" -> moveSelection(0, -1)
                "DPadDown", "ArrowDown", "Down" -> moveSelection(0, 1)
            }
            overlay.invalidate()
        }
        return true
    }

    private fun moveSelection(x: Int, y: Int) {
        if (y > 0 && selected / grid.columns == grid.rows - 1) {
            toggle.requestFocus()
            return
        }
        val column = (selected % grid.columns + x).coerceIn(0, grid.columns - 1)
        val row = (selected / grid.columns + y).coerceIn(0, grid.rows - 1)
        selected = row * grid.columns + column
        overlay.invalidate()
    }

    fun motion(event: MotionEvent): Boolean {
        if (!event.isFromSource(android.view.InputDevice.SOURCE_JOYSTICK)) return false
        val before = editing
        val left = maxOf(event.getAxisValue(MotionEvent.AXIS_LTRIGGER), event.getAxisValue(MotionEvent.AXIS_BRAKE))
        val right = maxOf(event.getAxisValue(MotionEvent.AXIS_RTRIGGER), event.getAxisValue(MotionEvent.AXIS_GAS))
        if (event.device?.getMotionRange(MotionEvent.AXIS_LTRIGGER) != null ||
            event.device?.getMotionRange(MotionEvent.AXIS_BRAKE) != null) chord(2, left > .5f)
        if (event.device?.getMotionRange(MotionEvent.AXIS_RTRIGGER) != null ||
            event.device?.getMotionRange(MotionEvent.AXIS_GAS) != null) chord(3, right > .5f)
        val wasWaiting = waiting
        if (editing && wasWaiting && before) {
            val x = event.getAxisValue(MotionEvent.AXIS_X)
            val y = event.getAxisValue(MotionEvent.AXIS_Y)
            val z = event.getAxisValue(MotionEvent.AXIS_Z)
            val rz = event.getAxisValue(MotionEvent.AXIS_RZ)
            val axes = listOf("LeftStickLeft" to -x, "LeftStickRight" to x,
                "LeftStickUp" to -y, "LeftStickDown" to y, "RightStickLeft" to -z,
                "RightStickRight" to z, "RightStickUp" to -rz, "RightStickDown" to rz)
            axes.maxByOrNull { it.second }?.takeIf { it.second > .5f }
                ?.let { assign(GridInput("controller", it.first)) }
        }
        if (editing && !wasWaiting && before) {
            val x = event.getAxisValue(MotionEvent.AXIS_HAT_X).takeIf { kotlin.math.abs(it) > .5f }
                ?: event.getAxisValue(MotionEvent.AXIS_X)
            val y = event.getAxisValue(MotionEvent.AXIS_HAT_Y).takeIf { kotlin.math.abs(it) > .5f }
                ?: event.getAxisValue(MotionEvent.AXIS_Y)
            val direction = when {
                x < -.5f -> 1
                x > .5f -> 2
                y < -.5f -> 3
                y > .5f -> 4
                else -> 0
            }
            if (direction != 0 && (direction != navigationDirection || event.eventTime >= nextNavigation)) {
                when (direction) {
                    1 -> moveSelection(-1, 0)
                    2 -> moveSelection(1, 0)
                    3 -> moveSelection(0, -1)
                    4 -> moveSelection(0, 1)
                }
                nextNavigation = event.eventTime + 180
            }
            navigationDirection = direction
        }
        return before || editing
    }
}
