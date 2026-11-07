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
            return "$label: ${state.shortFor(input,true,gizmondo)}"
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
        root.addView(text("Keyboard & controller", 22f))
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
        body.addView(text("Preview device"))
        body.addView(profiles)
        body.addView(
            text(
                "Select an input to inspect or change it. The preview does not change a game's profile. Save changes, then relaunch a running game to apply the updated bindings."
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
        for (pad in listOf(false, true)) section(body, pad)
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
