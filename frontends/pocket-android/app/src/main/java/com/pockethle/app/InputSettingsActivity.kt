package com.pockethle.app

import android.app.Dialog
import android.graphics.Color
import android.os.Bundle
import android.view.*
import android.widget.*
import androidx.appcompat.app.AppCompatActivity
import org.json.JSONObject

/** The desktop editor's two sections, adapted to touch and narrow screens. */
class InputSettingsActivity : AppCompatActivity() {
    private val controllerOnly: Boolean
        get() = intent.getBooleanExtra("controller", false)

    private val liveInput = InputLiveState()
    private val livePictures = mutableListOf<InputSkinView>()
    private val liveRows = mutableMapOf<String, Button>()
    private var listening = false
    private val inputListener =
        object : android.hardware.input.InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(id: Int) {
                if (::state.isInitialized) render()
            }

            override fun onInputDeviceChanged(id: Int) {
                liveInput.removeDevice(id)
                refreshLive()
            }

            override fun onInputDeviceRemoved(id: Int) {
                captureDialog?.dismiss()
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
        captureDialog?.dismiss()
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

    private fun recordKey(event: KeyEvent): Boolean {
        val pad = controllerEvent(event)
        val name =
            if (pad) InputBindings.controllerName(event.keyCode)
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
        }
        return pad || name != null
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        val recognized = recordKey(event)
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
    private var zoomKeyboard = false
    private var captureDialog: Dialog? = null
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
                        listOf("\nHold: ${state.short(it)}" to InputSkinView.LONG_PRESS)
                    } ?: emptyList())
            )
        val parts = mutableListOf(label + ": " to Color.LTGRAY)
        DeckInputPolicy.tap(input)?.let {
            parts.add(DeckInputPolicy.label(it) to InputSkinView.PURPLE)
        }
        DeckInputPolicy.hold(input, gizmondo)?.let {
            parts.add("\nHold: ${DeckInputPolicy.short(it)}" to InputSkinView.LONG_PRESS)
        }
        return spans(parts)
    }

    override fun onStop() {
        captureDialog?.dismiss()
        captureDialog = null
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
                            "Pressed input" to InputSkinView.LIVE,
                        )
                    )
            }
        )
        body.addView(
            button("Assign an input…") { quickAssign() },
            LinearLayout.LayoutParams(-1, dp(56)),
        )
        body.addView(
            text(
                "Press and release an input, then choose a device command. Hold for 0.65 s to assign a long press."
            )
        )
        section(body, controllerOnly)
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
        inspector(body, pad)
        val inputs = (diagram.controls.mapNotNull { it.input } + state.savedInputs(pad)).distinct()
        val missing =
            state.visibleButtons(gizmondo).filter { target ->
                !state.assigned(target, pad, gizmondo, inputs)
            }
        if (missing.isNotEmpty())
            body.addView(
                button("Unassigned commands (${missing.size})") {
                    android.app.AlertDialog.Builder(this)
                        .setTitle("Choose a command to assign")
                        .setItems(missing.map { state.label(it) }.toTypedArray()) { _, index ->
                            capture(pad, missing[index])
                        }
                        .setNegativeButton("Cancel", null)
                        .show()
                }
            )
    }

    private fun inspector(body: LinearLayout, pad: Boolean) {
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
        panel.addView(text("Assignment", 18f))
        val longTarget = state.longTarget(input, pad, gizmondo)
        if (longTarget != null) {
            panel.addView(
                text("Hold: ${state.label(longTarget)}").apply {
                    setTextColor(InputSkinView.LONG_PRESS)
                }
            )
            panel.addView(
                button("Clear long press") {
                    state.assignLong(input, pad, null, gizmondo)
                    render()
                }
            )
        }
        val target = state.assignment(input, pad, gizmondo)
        if (pad && input == "Misc1")
            panel.addView(
                text("Extra is a desktop-only input in the current Android adapter.", 12f)
            )
        val physical =
            if (pad)
                InputSkinView.loadControls(this, true).firstOrNull { it.input == input }?.label
                    ?: input
            else input
        panel.addView(text("Selected ${if(pad) "control" else "key"}: $physical", 17f))
        if (target == "host") {
            val tap = if (pad) DeckInputPolicy.tap(input) else DeckInputPolicy.key(input)
            tap?.let {
                panel.addView(
                    text("Tap: ${DeckInputPolicy.label(it)}").apply {
                        setTextColor(InputSkinView.PURPLE)
                    }
                )
            }
            if (pad)
                DeckInputPolicy.hold(input, gizmondo)?.let {
                    panel.addView(
                        text("Hold: ${DeckInputPolicy.label(it)}").apply {
                            setTextColor(InputSkinView.LONG_PRESS)
                        }
                    )
                }
        } else
            panel.addView(
                text("Current: ${state.label(target)}").apply {
                    setTextColor(InputSkinView.color(target))
                }
            )
        panel.addView(
            button("Choose input") {
                val inputs =
                    (InputSkinView.loadControls(this, pad).mapNotNull { it.input } +
                            state.savedInputs(pad))
                        .distinct()
                        .filter { !(pad && it == "Misc1") }
                        .sorted()
                android.app.AlertDialog.Builder(this)
                    .setTitle("Choose input")
                    .setItems(inputs.toTypedArray()) { _, i ->
                        selected[pad] = inputs[i]
                        render()
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
        )
        if (
            target == "host" &&
                (DeckInputPolicy.key(input) == DeckInputPolicy.Action.RECORDING ||
                    (pad &&
                        DeckInputPolicy.hold(input, gizmondo) == DeckInputPolicy.Action.RECORDING))
        ) {
            panel.addView(text("* Recording is currently available on desktop only.", 12f))
        }
        if (state.fixed(input, pad, gizmondo)) {
            panel.addView(
                text(
                    if (target == "host") "Built-in Deck shortcut."
                    else "Built-in device mapping (Deck reference)."
                )
            )
            return
        }
        panel.addView(
            button("Change assignment") {
                val targets =
                    listOf<String?>(null) +
                        state.visibleButtons(gizmondo) +
                        if (gizmondo) emptyList() else (0..3).map { "stylus:$it" }
                android.app.AlertDialog.Builder(this)
                    .setTitle("Assign $physical")
                    .setItems(targets.map { state.label(it) }.toTypedArray()) { _, i ->
                        state.assign(input, pad, targets[i], gizmondo)
                        render()
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
        )
        val row = LinearLayout(this)
        row.addView(
            button("Clear") {
                state.assign(input, pad, null, gizmondo)
                render()
            },
            LinearLayout.LayoutParams(0, dp(48), 1f),
        )
        row.addView(
            button("Press an input…") { if (target != null) capture(pad, target) }
                .apply {
                    isEnabled = target != null
                    alpha = if (target != null) 1f else .45f
                },
            LinearLayout.LayoutParams(0, dp(48), 1f),
        )
        panel.addView(row)
    }

    private fun quickAssign() {
        captureDialog?.dismiss()
        val pad = controllerOnly
        val targets = listOf<String?>(null) + state.visibleButtons(gizmondo)
        val dialog =
            object : Dialog(this) {
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
                        notice = "This input has a built-in PocketHLE shortcut. Choose another input."
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
                            "${if(long) "Long" else "Short"} press: $name → ${state.label(target)}"
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
                                if (input == null)
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
                            if (stage == 1) targets.map { state.label(it) }
                            else listOf("Assign another input", "Save and finish", "Back to editor")
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

                override fun dispatchKeyEvent(event: KeyEvent): Boolean {
                    recordKey(event)
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
                    val candidate = recordMotion(event)?.takeIf { it.second > .6f }?.first
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
        captureDialog = dialog
        dialog.setOnDismissListener {
            captureDialog = null
            liveInput.clear()
            refreshLive()
        }
        dialog.show()
        dialog.draw()
    }

    private fun capture(pad: Boolean, target: String) {
        val dialog =
            object : Dialog(this) {
                fun accept(name: String): Boolean {
                    if (state.assign(name, pad, target, gizmondo)) {
                        selected[pad] = if (pad) InputBindings.controllerCanonical(name) else name
                        dismiss()
                        render()
                    }
                    return true
                }

                override fun dispatchKeyEvent(event: KeyEvent): Boolean {
                    recordKey(event)
                    if (event.keyCode == KeyEvent.KEYCODE_BACK) return super.dispatchKeyEvent(event)
                    if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return true
                    val sources = event.device?.sources ?: event.source
                    val controller =
                        sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
                            sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
                    if (controller != pad) return true
                    val name =
                        if (pad) InputBindings.controllerName(event.keyCode)
                        else InputBindings.keyName(event)
                    return name?.let { accept(it) } ?: true
                }

                override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
                    if (
                        !pad ||
                            event.source and InputDevice.SOURCE_JOYSTICK !=
                                InputDevice.SOURCE_JOYSTICK
                    )
                        return super.dispatchGenericMotionEvent(event)
                    recordMotion(event)?.takeIf { it.second > .6f }?.let { accept(it.first) }
                    return true
                }
            }
        val content =
            column().apply {
                setPadding(dp(20), dp(20), dp(20), dp(20))
                setBackgroundColor(Color.rgb(30, 33, 38))
            }
        content.addView(
            text(
                "Press ${if(pad) "a controller input" else "a keyboard key"} for ${state.label(target)}",
                18f,
            )
        )
        content.addView(text("Reserved Deck controls keep their built-in assignment.", 12f))
        content.addView(button("Cancel") { dialog.dismiss() })
        dialog.setContentView(content)
        captureDialog = dialog
        dialog.setOnDismissListener { captureDialog = null }
        dialog.show()
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
