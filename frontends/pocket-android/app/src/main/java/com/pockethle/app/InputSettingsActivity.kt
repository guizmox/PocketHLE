package com.pockethle.app

import android.app.Dialog
import android.graphics.Color
import android.os.Bundle
import android.view.*
import android.widget.*
import org.json.JSONObject

/** Shared input-first assistant with read-only skin feedback and selected-input summary. */
class InputSettingsActivity : HostKeyActivity() {
    private val controllerOnly: Boolean
        get() = intent.getBooleanExtra("controller", false)

    private val shortcuts = AssignmentShortcuts()
    private val liveInput = InputLiveState()
    override fun reservedInputAttempt() {
        if(assignmentDialog!=null && !shortcuts.awaitingRelease) android.widget.Toast.makeText(this,"This input is reserved by PocketHLE. Choose another input.",android.widget.Toast.LENGTH_SHORT).show()
    }
    private val livePictures = mutableListOf<InputSkinView>()
    private val liveRows = mutableMapOf<String, Button>()
    private var listening = false
    private val inputListener =
        object : android.hardware.input.InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(id: Int) {
                if (::state.isInitialized) render()
            }

            override fun onInputDeviceChanged(id: Int) {
                shortcuts.removeDevice(id)
                liveInput.removeDevice(id)
                refreshLive()
            }

            override fun onInputDeviceRemoved(id: Int) {
                assignmentDialog?.dismiss()
                shortcuts.removeDevice(id)
                liveInput.removeDevice(id)
                if (::state.isInitialized) render()
            }
        }

    override fun onResume() {
        super.onResume()
        if (::state.isInitialized) {
            liveInput.clear()
            render()
            (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager)
                .registerInputDeviceListener(inputListener, null)
            listening = true
        }
    }

    override fun onPause() {
        assignmentDialog?.dismiss()
        shortcuts.clear()
        if (listening) {
            (getSystemService(INPUT_SERVICE) as android.hardware.input.InputManager)
                .unregisterInputDeviceListener(inputListener)
            listening = false
        }
        liveInput.clear()
        refreshLive()
        super.onPause()
    }

    override fun onWindowFocusChanged(focused: Boolean) {
        super.onWindowFocusChanged(focused)
        if (!focused) {
            if (assignmentDialog == null) shortcuts.clear()
            liveInput.clear()
            refreshLive()
        }
    }

    private fun refreshLive() {
        livePictures.forEach { it.invalidate() }
        liveRows.forEach { (name, row) -> row.isActivated = liveInput.isDown(name, true) }
    }

    private fun controllerEvent(event: KeyEvent): Boolean {
        val sources = event.device?.sources ?: event.source
        return sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
            sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
    }

    private fun inputName(event: KeyEvent): String? {
        val pad = controllerEvent(event)
        return if (pad) InputBindings.controllerName(event.keyCode)
        else
            when (event.keyCode) {
                KeyEvent.KEYCODE_SHIFT_LEFT,
                KeyEvent.KEYCODE_SHIFT_RIGHT -> "Shift"
                KeyEvent.KEYCODE_CTRL_LEFT,
                KeyEvent.KEYCODE_CTRL_RIGHT -> "Ctrl"
                KeyEvent.KEYCODE_ALT_LEFT,
                KeyEvent.KEYCODE_ALT_RIGHT -> "Alt"
                KeyEvent.KEYCODE_META_LEFT,
                KeyEvent.KEYCODE_META_RIGHT -> "Super"
                KeyEvent.KEYCODE_CAPS_LOCK -> "CapsLock"
                KeyEvent.KEYCODE_MENU -> "ContextMenu"
                KeyEvent.KEYCODE_GRAVE -> "Backtick"
                KeyEvent.KEYCODE_APOSTROPHE -> "Quote"
                else -> InputBindings.keyName(event)
            }
    }

    private fun recordKey(event: KeyEvent): Boolean {
        val pad = controllerEvent(event)
        val name = inputName(event)
        if (
            name != null &&
                (event.action == KeyEvent.ACTION_DOWN || event.action == KeyEvent.ACTION_UP)
        ) {
            liveInput.key(
                event.deviceId,
                pad,
                event.keyCode,
                name,
                event.action == KeyEvent.ACTION_DOWN,
            )
            refreshLive()
            val activate =
                shortcuts.key(
                    device = event.deviceId,
                    pad = pad,
                    name = name,
                    down = event.action == KeyEvent.ACTION_DOWN,
                    repeat = event.repeatCount != 0,
                    ctrlPressed = event.isCtrlPressed,
                    controllerScreen = controllerOnly,
                    active = assignmentDialog != null,
                    extraModifiers =
                        event.isAltPressed || event.isShiftPressed || event.isMetaPressed,
                )
            if (activate && ::state.isInitialized) quickAssign()
        }
        return pad || name != null
    }

    override fun onHostFilterChanged(filter: String) {
        if (::state.isInitialized) state.json.put("upscale_filter", filter)
    }

    override fun onHostFullscreenChanged() {
        assignmentDialog?.window?.let { applyHostFullscreen(it) }
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val recognized = recordKey(event)
        if (handleHostKeyEvent(event)) return true
        if (event.keyCode == KeyEvent.KEYCODE_BACK && !controllerEvent(event))
            return super.dispatchKeyEvent(event)
        return if (recognized) true else super.dispatchKeyEvent(event)
    }

    private fun recordMotion(event: MotionEvent): Pair<String, Float>? {
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) {
            liveInput.removeDevice(event.deviceId)
            refreshLive()
            return null
        }
        var strongest: Pair<String, Float>? = null
        fun available(preferred: Int, fallback: Int) =
            if (
                event.device == null ||
                    event.device?.getMotionRange(preferred, event.source) != null
            )
                preferred
            else fallback
        fun axis(id: Int, negative: String?, positive: String, trigger: Boolean = false) {
            val value = event.getAxisValue(id)
            liveInput.axis(
                event.deviceId,
                id,
                value,
                negative,
                positive,
                if (trigger) .5f else .25f,
                if (trigger) .3f else .15f,
            )
            val candidate = if (value >= 0) positive to value else negative?.let { it to -value }
            if (
                candidate != null &&
                    candidate.second.isFinite() &&
                    candidate.second > (strongest?.second ?: 0f)
            )
                strongest = candidate
        }
        axis(MotionEvent.AXIS_X, "LeftStickLeft", "LeftStickRight")
        axis(MotionEvent.AXIS_Y, "LeftStickUp", "LeftStickDown")
        axis(
            available(MotionEvent.AXIS_Z, MotionEvent.AXIS_RX),
            "RightStickLeft",
            "RightStickRight",
        )
        axis(available(MotionEvent.AXIS_RZ, MotionEvent.AXIS_RY), "RightStickUp", "RightStickDown")
        axis(MotionEvent.AXIS_HAT_X, "DPadLeft", "DPadRight")
        axis(MotionEvent.AXIS_HAT_Y, "DPadUp", "DPadDown")
        axis(
            available(MotionEvent.AXIS_LTRIGGER, MotionEvent.AXIS_BRAKE),
            null,
            "LeftTrigger2",
            true,
        )
        axis(
            available(MotionEvent.AXIS_RTRIGGER, MotionEvent.AXIS_GAS),
            null,
            "RightTrigger2",
            true,
        )
        refreshLive()
        return strongest
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK) {
            recordMotion(event)
            return true
        }
        return super.dispatchGenericMotionEvent(event)
    }

    private lateinit var state: InputEditorState
    private lateinit var rootDir: String
    private var gizmondo = false

    override fun hostGizmondo() = gizmondo

    private var zoomKeyboard = false
    private var assignmentDialog: Dialog? = null
    private var scroll: ScrollView? = null
    private var keyboardScroll: HorizontalScrollView? = null
    private val selected = mutableMapOf(false to "Z", true to "DPadUp")

    private fun dp(n: Int) = (n * resources.displayMetrics.density).toInt()

    private fun column() = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

    private fun text(value: String, size: Float = 14f) =
        TextView(this).apply {
            text = value
            textSize = size
            setTextColor(Color.LTGRAY)
            setPadding(0, dp(5), 0, dp(5))
        }

    private fun button(value: CharSequence, action: () -> Unit) =
        Button(this).apply {
            text = value
            setAllCaps(false)
            textSize = 14f
            setTextColor(Color.WHITE)
            setPadding(dp(8), 0, dp(8), 0)
            fun shape(color: Int) =
                android.graphics.drawable.GradientDrawable().apply {
                    setColor(color)
                    setCornerRadius(dp(4).toFloat())
                }
            background =
                android.graphics.drawable.StateListDrawable().apply {
                    addState(
                        intArrayOf(android.R.attr.state_activated),
                        shape(Color.rgb(30, 62, 72)).apply { setStroke(dp(2), InputSkinView.LIVE) },
                    )
                    addState(intArrayOf(android.R.attr.state_pressed), shape(Color.rgb(55, 76, 99)))
                    addState(
                        intArrayOf(android.R.attr.state_selected),
                        shape(Color.rgb(42, 66, 88)),
                    )
                    addState(intArrayOf(), shape(Color.rgb(40, 45, 53)))
                }
            setOnClickListener { action() }
        }

    private fun spans(parts: List<Pair<String, Int>>): android.text.SpannableStringBuilder {
        val result = android.text.SpannableStringBuilder()
        for ((value, color) in parts) {
            val start = result.length
            result.append(value)
            result.setSpan(
                android.text.style.ForegroundColorSpan(color),
                start,
                result.length,
                android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE,
            )
        }
        return result
    }

    private fun bindingText(input: String, label: String): CharSequence {
        if (input == "Misc1") return "Extra (desktop)"
        if (state.assignment(input, true, gizmondo) != "host")
            return spans(
                listOf(
                    "$label: ${state.shortFor(input,true,gizmondo)}" to
                        InputSkinView.color(state.assignment(input, true, gizmondo))
                ) +
                    (state.longTarget(input, true, gizmondo)?.let {
                        listOf("\nHold: ${state.short(it, gizmondo)}" to InputSkinView.LONG_PRESS)
                    } ?: emptyList())
            )
        val parts = mutableListOf(label + ": " to Color.LTGRAY)
        DeckInputPolicy.tap(input)?.let {
            parts.add(DeckInputPolicy.label(it) to InputSkinView.PURPLE)
        }
        val customHold = state.longTarget(input, true, gizmondo)
        if (customHold != null) {
            parts.add("\nHold: ${state.short(customHold, gizmondo)}" to InputSkinView.LONG_PRESS)
        } else
            DeckInputPolicy.hold(input, gizmondo)?.let {
                parts.add("\nHold: ${DeckInputPolicy.short(it)}" to InputSkinView.LONG_PRESS)
            }
        return spans(parts)
    }

    override fun onStop() {
        assignmentDialog?.dismiss()
        assignmentDialog = null
        super.onStop()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        rootDir = LibraryPaths.root(this)
        try {
            val raw = savedInstanceState?.getString("draft") ?: NativeBridge.readConfig(rootDir)
            val json = JSONObject(raw)
            if (json.has("ok") && !json.optBoolean("ok")) error(json.optString("error"))
            state = InputEditorState(raw)
        } catch (e: Exception) {
            Toast.makeText(this, e.message ?: "Could not read settings", Toast.LENGTH_LONG).show()
            finish()
            return
        }
        gizmondo = savedInstanceState?.getBoolean("gizmondo") ?: false
        zoomKeyboard = savedInstanceState?.getBoolean("zoom") ?: false
        selected[false] = savedInstanceState?.getString("key") ?: "Z"
        selected[true] = savedInstanceState?.getString("control") ?: "DPadUp"
        render()
    }

    override fun onSaveInstanceState(out: Bundle) {
        super.onSaveInstanceState(out)
        if (::state.isInitialized) out.putString("draft", state.json.toString())
        out.putBoolean("gizmondo", gizmondo)
        out.putBoolean("zoom", zoomKeyboard)
        out.putString("key", selected[false])
        out.putString("control", selected[true])
    }

    private fun render() {
        livePictures.clear()
        liveRows.clear()
        val oldY = scroll?.scrollY ?: 0
        val oldX = keyboardScroll?.scrollX ?: 0
        val root =
            column().apply {
                setBackgroundColor(Color.rgb(23, 25, 29))
                setPadding(dp(12), dp(8), dp(12), dp(8))
            }
        root.addView(
            text(if (controllerOnly) "External controller bindings" else "Keyboard bindings", 22f)
        )
        val actions = LinearLayout(this)
        actions.addView(button("Save") { save() }, LinearLayout.LayoutParams(0, dp(48), 1f))
        actions.addView(button("Cancel") { finish() }, LinearLayout.LayoutParams(0, dp(48), 1f))
        root.addView(actions)
        val body = column()
        val profiles = RadioGroup(this).apply { orientation = RadioGroup.HORIZONTAL }
        listOf(false to "Pocket PC", true to "Gizmondo").forEachIndexed { index, (value, label) ->
            profiles.addView(
                RadioButton(this).apply {
                    id = 100 + index
                    text = label
                    setTextColor(Color.WHITE)
                    isChecked = gizmondo == value
                }
            )
        }
        profiles.setOnCheckedChangeListener { _, id ->
            gizmondo = id == 101
            render()
        }
        body.addView(text("Device profile"))
        body.addView(profiles)
        body.addView(
            text(
                "Select an input to inspect it, or use Assign an input. Guest button assignments apply to the selected device profile. Save changes, then relaunch a running game to apply them."
            )
        )
        body.addView(
            text("", 12f).apply {
                text =
                    spans(
                        listOf(
                            "Game · " to InputSkinView.BLUE,
                            "Stylus · " to InputSkinView.GREEN,
                            "Shortcut · " to InputSkinView.PURPLE,
                            "Long press · " to InputSkinView.LONG_PRESS,
                            "Unassigned · " to Color.GRAY,
                            "Pressed input · " to InputSkinView.LIVE,
                            "Reserved input" to InputSkinView.RESERVED,
                        )
                    )
            }
        )
        body.addView(
            button(
                if (controllerOnly) "Assign an input… (L3 + R3)"
                else "Assign an input… (Ctrl+Enter)"
            ) {
                quickAssign()
            },
            LinearLayout.LayoutParams(-1, dp(56)),
        )
        body.addView(
            text(
                "Press and release an input, then choose a device command. Hold for 0.65 s to assign a long press."
            )
        )
        section(body, controllerOnly)
        if(!gizmondo) {
            if(controllerOnly) body.addView(text("Double-click R3 to switch stylus mode. Stick: automatic drags, single click unused. Pointer: move cursor, single click to tap. Right stick and R3 are reserved."))
            if(!controllerOnly) body.addView(text("Stylus click / mode: P by default. Double-press to switch mode; single press taps in Pointer mode. Reassign this key and stylus directions with Assign an input. Current stylus keys cannot be used in per-game grids."))
            val speedLabel=text("Stylus speed (pixels): ${state.stylus.optInt("speed",240).coerceIn(30,1200)}")
            body.addView(speedLabel)
            body.addView(android.widget.SeekBar(this).apply {
                max=1170;progress=(state.stylus.optInt("speed",240).coerceIn(30,1200)-30)
                setOnSeekBarChangeListener(object: android.widget.SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(bar:android.widget.SeekBar?,value:Int,fromUser:Boolean) { if(fromUser) { state.stylus.put("speed",value+30);speedLabel.text="Stylus speed (pixels): ${value+30}" } }
                    override fun onStartTrackingTouch(bar:android.widget.SeekBar?) {}
                    override fun onStopTrackingTouch(bar:android.widget.SeekBar?) {}
                })
            })
        }

        scroll =
            ScrollView(this).apply {
                isFillViewport = true
                addView(body)
            }
        root.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        setContentView(root)
        scroll?.post {
            scroll?.scrollTo(0, oldY)
            keyboardScroll?.scrollTo(oldX, 0)
        }
    }

    private fun section(body: LinearLayout, pad: Boolean) {
        body.addView(
            View(this).apply { setBackgroundColor(Color.rgb(65, 68, 74)) },
            LinearLayout.LayoutParams(-1, dp(1)).apply { topMargin = dp(18) },
        )
        body.addView(text(if (pad) "Controller" else "Keyboard", 20f))
        val diagram =
            InputSkinView(this, pad, state, gizmondo, selected[pad], liveInput) { input ->
                selected[pad] = input
                render()
            }
        livePictures.add(diagram)
        if (pad) {
            val devices =
                InputDevice.getDeviceIds()
                    .toList()
                    .mapNotNull { InputDevice.getDevice(it) }
                    .filter {
                        it.sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
                            it.sources and InputDevice.SOURCE_JOYSTICK ==
                                InputDevice.SOURCE_JOYSTICK
                    }
            body.addView(
                text(
                    if (devices.isEmpty())
                        "No controller detected. Connect through USB or Android Bluetooth settings."
                    else devices.joinToString(" · ") { it.name }
                )
            )
            body.addView(diagram, LinearLayout.LayoutParams(-1, -2))
            val controls = diagram.controls
            for (pair in
                controls.take(13).zip(controls.drop(13)).map { listOf(it.first, it.second) }) {
                val row = LinearLayout(this)
                for (control in pair) row.addView(
                    button(bindingText(control.input!!, control.label)) {
                            selected[true] = control.input!!
                            render()
                        }
                        .apply {
                            liveRows[control.input!!] = this
                            isActivated = liveInput.isDown(control.input!!, true)
                            isSelected = selected[true] == control.input
                            isEnabled = control.input != "Misc1"
                            alpha = if (isEnabled) 1f else .45f
                            setTextColor(
                                InputSkinView.color(
                                    state.assignment(control.input!!, true, gizmondo)
                                )
                            )
                        },
                    LinearLayout.LayoutParams(0, dp(48), 1f),
                )
                body.addView(row)
            }
        } else {
            body.addView(
                button(if (zoomKeyboard) "Fit keyboard" else "Enlarge keyboard") {
                    zoomKeyboard = !zoomKeyboard
                    render()
                }
            )
            val skinWidth =
                if (zoomKeyboard) dp(1100)
                else
                    (resources.configuration.screenWidthDp * resources.displayMetrics.density)
                        .toInt() - dp(24)
            val skinHeight = (skinWidth * .34f).toInt()
            keyboardScroll =
                HorizontalScrollView(this).apply {
                    isFillViewport = !zoomKeyboard
                    addView(diagram, android.widget.FrameLayout.LayoutParams(skinWidth, skinHeight))
                }
            body.addView(keyboardScroll, LinearLayout.LayoutParams(-1, skinHeight))
            body.addView(
                text(
                    if (zoomKeyboard) "Swipe horizontally to reach every key."
                    else "AZERTY keyboard. Enlarge for easier touch selection.",
                    12f,
                )
            )
        }
        body.addView(
            button(if (pad) "Reset controller defaults" else "Reset keys to defaults") {
                val defaults =
                    JSONObject(
                        assets.open("deck-input-defaults.json").bufferedReader().use {
                            it.readText()
                        }
                    )
                state.reset(pad, defaults)
                render()
            }
        )
        summary(body, pad)
        val inputs = (diagram.controls.mapNotNull { it.input } + state.savedInputs(pad)).distinct()
        val missing =
            state.visibleButtons(gizmondo).filter { !pad || it!="stylus_click" }.filter { target ->
                !state.assigned(target, pad, gizmondo, inputs)
            }
        if (missing.isNotEmpty())
            body.addView(
                text(
                        "Unassigned commands: ${missing.joinToString {state.label(it, gizmondo)}}",
                        12f,
                    )
                    .apply { setTextColor(Color.GRAY) }
            )
    }

    private fun summary(body: LinearLayout, pad: Boolean) {
        val input = selected[pad] ?: return
        val panel =
            column().apply {
                setPadding(dp(12), dp(8), dp(12), dp(12))
                setBackgroundColor(Color.rgb(31, 35, 41))
            }
        body.addView(
            panel,
            LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(12)
                bottomMargin = dp(8)
            },
        )
        val physical =
            if (pad)
                InputSkinView.loadControls(this, true).firstOrNull { it.input == input }?.label
                    ?: input
            else input
        panel.addView(text("Selected input: $physical", 18f))
        if (pad && DeckInputPolicy.stylusReserved(input,gizmondo)) {
            panel.addView(text("Reserved stylus control"))
            panel.addView(text("Double-click R3 to switch stylus mode."))
            panel.addView(text("Stick: automatic drags; single click unused."))
            panel.addView(text("Pointer: move cursor; single click to tap."))
            return
        }
        val target = state.assignment(input, pad, gizmondo)
        if (target == "host") {
            val tap = if (pad) DeckInputPolicy.tap(input) else DeckInputPolicy.key(input)
            tap?.let {
                panel.addView(
                    text("Short press: ${DeckInputPolicy.label(it)}").apply {
                        setTextColor(InputSkinView.PURPLE)
                    }
                )
            }
            if (pad)
                DeckInputPolicy.hold(input, gizmondo)?.let {
                    panel.addView(
                        text("Long press: ${DeckInputPolicy.label(it)}").apply {
                            setTextColor(InputSkinView.LONG_PRESS)
                        }
                    )
                }
        } else {
            panel.addView(
                text("Short press: ${state.label(target, gizmondo)}").apply {
                    setTextColor(InputSkinView.color(target))
                }
            )
            val long = state.longTarget(input, pad, gizmondo)
            panel.addView(
                text("Long press: ${state.label(long, gizmondo)}").apply {
                    setTextColor(if (long == null) Color.GRAY else InputSkinView.LONG_PRESS)
                }
            )
        }
        if (pad && input == "Misc1")
            panel.addView(
                text("Extra is a desktop-only input in the current Android adapter.", 12f)
            )
    }

    private fun quickAssign() {
        assignmentDialog?.dismiss()
        val pad = controllerOnly
        val dialog =
            object : Dialog(this) {
                val targets: List<String?>
                    get() = state.targets(pad, input, long, gizmondo)

                var stage = 0 // capture, choose guest action, then continue/save
                var input: String? = null
                var started = 0L
                var long = false
                var index = 0
                var axisNav: String? = null
                var notice = ""

                fun begin() {
                    stage = 0
                    input = null
                    index = 0
                    draw()
                }

                fun choose(name: String, now: Long) {
                    input =
                        if (pad) InputBindings.controllerCanonical(name)
                        else InputBindings.canonical(name)
                    long = now - started >= InputGestures.HOLD_MS
                    if (
                        state.fixed(input!!, pad, gizmondo) || (!pad && long && input == "Escape")
                    ) {
                        notice =
                            "This input is reserved by PocketHLE. Choose another input."
                        begin()
                        return
                    }
                    stage = 1
                    index = 0
                    draw()
                }

                fun applyChoice() {
                    if (stage == 1) {
                        val name = input ?: return
                        val target = targets[index]
                        val ok =
                            if (long) state.assignLong(name, pad, target, gizmondo)
                            else state.assign(name, pad, target, gizmondo)
                        if (!ok) {
                            notice = "This input is reserved. Choose another input."
                            begin()
                            return
                        }
                        selected[pad] = name
                        notice =
                            "${if(long) "Long" else "Short"} press: $name → ${state.label(target, gizmondo)}"
                        stage = 2
                        index = 0
                        render()
                        draw()
                    } else if (stage == 2)
                        when (index) {
                            0 -> begin()
                            1 -> {
                                dismiss()
                                save()
                            }
                            else -> dismiss()
                        }
                }

                fun navigate(name: String) {
                    if (stage == 0) return
                    val size = if (stage == 1) targets.size else 3
                    when (InputBindings.controllerCanonical(name)) {
                        "Up",
                        "DPadUp",
                        "LeftStickUp" -> index = (index + size - 1) % size
                        "Down",
                        "DPadDown",
                        "LeftStickDown" -> index = (index + 1) % size
                        "Enter",
                        "South" -> {
                            applyChoice()
                            return
                        }
                        "Escape",
                        "East" -> {
                            dismiss()
                            return
                        }
                        else -> return
                    }
                    draw()
                }

                fun draw() {
                    val content =
                        column().apply {
                            setPadding(dp(20), dp(12), dp(20), dp(12))
                            setBackgroundColor(Color.rgb(23, 25, 29))
                        }
                    content.addView(text("Assign an input", 22f))
                    if (notice.isNotEmpty())
                        content.addView(text(notice).apply { setTextColor(InputSkinView.LIVE) })
                    if (stage == 0) {
                        content.addView(
                            text(
                                if (shortcuts.awaitingRelease) "Release the shortcut buttons first…"
                                else if (input == null)
                                    "Press and release a ${if(pad) "controller button" else "keyboard key"}…"
                                else "$input — release to continue"
                            )
                        )
                        content.addView(
                            text("Hold for 0.65 s for a long press. Android Back cancels.")
                        )
                    } else {
                        if (stage == 1) {
                            content.addView(
                                text("$input · ${if(long) "Long" else "Short"} press", 20f)
                            )
                            content.addView(
                                text(
                                    "Up / Down to choose · Enter / A to apply · Escape / B to cancel"
                                )
                            )
                            if (long)
                                content.addView(
                                    text(
                                        "Adding a long action delays this input's short action until release."
                                    )
                                )
                        }
                        val choices =
                            if (stage == 1) targets.map { state.label(it, gizmondo) }
                            else
                                listOf(
                                    "Assign another input",
                                    "Save and finish",
                                    "Back to overview",
                                )
                        val list = column()
                        var selectedView: View? = null
                        choices.forEachIndexed { i, label ->
                            val item =
                                button(label) {
                                    index = i
                                    applyChoice()
                                }
                            item.isSelected = i == index
                            list.addView(item, LinearLayout.LayoutParams(-1, dp(48)))
                            if (i == index) selectedView = item
                        }
                        val scroller =
                            ScrollView(this@InputSettingsActivity).apply { addView(list) }
                        content.addView(scroller, LinearLayout.LayoutParams(-1, dp(300)))
                        scroller.post { selectedView?.let { scroller.smoothScrollTo(0, it.top) } }
                    }
                    content.addView(button("Cancel") { dismiss() })
                    setContentView(content)
                    window?.setLayout((resources.displayMetrics.widthPixels * .94f).toInt(), -2)
                }

                override fun onWindowFocusChanged(focused: Boolean) {
                    super.onWindowFocusChanged(focused)
                    if (!focused) dismiss()
                }

                override fun dispatchKeyEvent(event: KeyEvent): Boolean {
                    val waiting = shortcuts.awaitingRelease
                    recordKey(event)
                    if (handleHostKeyEvent(event)) return true
                    if (waiting) {
                        if (!shortcuts.awaitingRelease) draw()
                        return true
                    }
                    if (event.keyCode == KeyEvent.KEYCODE_BACK && !controllerEvent(event))
                        return super.dispatchKeyEvent(event)
                    val fromPad = controllerEvent(event)
                    val name =
                        if (fromPad) InputBindings.controllerName(event.keyCode)
                        else InputBindings.keyName(event)
                    if (stage != 0) {
                        if (
                            event.action == KeyEvent.ACTION_DOWN &&
                                event.repeatCount == 0 &&
                                name != null
                        )
                            navigate(name)
                        return true
                    }
                    if (fromPad != pad || name == null) return true
                    if (
                        event.action == KeyEvent.ACTION_DOWN &&
                            event.repeatCount == 0 &&
                            input == null
                    ) {
                        input = name
                        started = event.eventTime
                        draw()
                    } else if (
                        event.action == KeyEvent.ACTION_UP &&
                            input?.let {
                                if (pad)
                                    InputBindings.controllerCanonical(it) ==
                                        InputBindings.controllerCanonical(name)
                                else it == name
                            } == true
                    )
                        if (!pad || !liveInput.isDown(name, true)) choose(name, event.eventTime)
                    return true
                }

                override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
                    if (
                        event.source and InputDevice.SOURCE_JOYSTICK !=
                            InputDevice.SOURCE_JOYSTICK &&
                            event.source and InputDevice.SOURCE_GAMEPAD !=
                                InputDevice.SOURCE_GAMEPAD
                    )
                        return super.dispatchGenericMotionEvent(event)
                    val candidate = recordMotion(event)?.takeIf { it.second > .6f }?.first
                    if (shortcuts.awaitingRelease) return true
                    if (stage != 0) {
                        if (candidate != axisNav) {
                            axisNav = candidate
                            candidate?.let { navigate(it) }
                        }
                        return true
                    }
                    if (!pad) return true
                    if (input == null && candidate != null) {
                        input = InputBindings.controllerCanonical(candidate)
                        started = event.eventTime
                        draw()
                    } else
                        input?.let { if (!liveInput.isDown(it, true)) choose(it, event.eventTime) }
                    return true
                }
            }
        assignmentDialog = dialog
        dialog.setOnDismissListener {
            assignmentDialog = null
            shortcuts.clear()
            liveInput.clear()
            refreshLive()
        }
        dialog.show()
        dialog.window?.let { applyHostFullscreen(it) }
        dialog.draw()
    }

    private fun save() {
        val result =
            runCatching { JSONObject(NativeBridge.writeConfig(rootDir, state.json.toString())) }
                .getOrNull()
        if (result?.optBoolean("ok", false) == true) finish()
        else
            Toast.makeText(
                    this,
                    result?.optString("error") ?: "Could not save settings",
                    Toast.LENGTH_LONG,
                )
                .show()
    }
}
