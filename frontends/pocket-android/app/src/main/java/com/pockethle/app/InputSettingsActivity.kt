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

    private fun button(value: String, action: () -> Unit) =
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
                    addState(intArrayOf(android.R.attr.state_pressed), shape(Color.rgb(55, 76, 99)))
                    addState(
                        intArrayOf(android.R.attr.state_selected),
                        shape(Color.rgb(42, 66, 88)),
                    )
                    addState(intArrayOf(), shape(Color.rgb(40, 45, 53)))
                }
            setOnClickListener { action() }
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
                "Select an input to inspect or change it. The preview does not change a game's profile. Changes apply after Save."
            )
        )
        body.addView(text("Blue: game · green: stylus · purple: host · gray: unassigned", 12f))
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
            InputSkinView(this, pad, state, gizmondo, selected[pad]) { input ->
                selected[pad] = input
                render()
            }
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
                    button(
                            "${control.label}: ${state.short(state.assignment(control.input!!,true,gizmondo))}"
                        ) {
                            selected[true] = control.input!!
                            render()
                        }
                        .apply {
                            isSelected = selected[true] == control.input
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
        inspector(body, pad)
        val inputs = (diagram.controls.mapNotNull { it.input } + state.savedInputs(pad)).distinct()
        val missing =
            state.visibleButtons(gizmondo).filter { target ->
                inputs.none { state.assignment(it, pad, gizmondo) == target }
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
        val physical =
            if (pad)
                InputSkinView.loadControls(this, true).firstOrNull { it.input == input }?.label
                    ?: input
            else input
        panel.addView(text("Selected ${if(pad) "control" else "key"}: $physical", 17f))
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
        if (target == "host") {
            panel.addView(text("F10 is reserved for screenshots on Android."))
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
                        state.assign(input, pad, targets[i])
                        render()
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
        )
        val row = LinearLayout(this)
        row.addView(
            button("Clear") {
                state.assign(input, pad, null)
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
                    if (state.assign(name, pad, target)) {
                        selected[pad] = if (pad) InputBindings.controllerCanonical(name) else name
                        dismiss()
                        render()
                    }
                    return true
                }

                override fun dispatchKeyEvent(event: KeyEvent): Boolean {
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
                    val axes =
                        listOf(
                            "LeftStickRight" to event.getAxisValue(MotionEvent.AXIS_X),
                            "LeftStickLeft" to -event.getAxisValue(MotionEvent.AXIS_X),
                            "LeftStickDown" to event.getAxisValue(MotionEvent.AXIS_Y),
                            "LeftStickUp" to -event.getAxisValue(MotionEvent.AXIS_Y),
                            "RightStickRight" to event.getAxisValue(MotionEvent.AXIS_Z),
                            "RightStickLeft" to -event.getAxisValue(MotionEvent.AXIS_Z),
                            "RightStickDown" to event.getAxisValue(MotionEvent.AXIS_RZ),
                            "RightStickUp" to -event.getAxisValue(MotionEvent.AXIS_RZ),
                            "DPadRight" to event.getAxisValue(MotionEvent.AXIS_HAT_X),
                            "DPadLeft" to -event.getAxisValue(MotionEvent.AXIS_HAT_X),
                            "DPadDown" to event.getAxisValue(MotionEvent.AXIS_HAT_Y),
                            "DPadUp" to -event.getAxisValue(MotionEvent.AXIS_HAT_Y),
                            "LeftTrigger2" to
                                maxOf(
                                    event.getAxisValue(MotionEvent.AXIS_LTRIGGER),
                                    event.getAxisValue(MotionEvent.AXIS_BRAKE),
                                ),
                            "RightTrigger2" to
                                maxOf(
                                    event.getAxisValue(MotionEvent.AXIS_RTRIGGER),
                                    event.getAxisValue(MotionEvent.AXIS_GAS),
                                ),
                        )
                    axes
                        .maxByOrNull { it.second }
                        ?.takeIf { it.second > .6f }
                        ?.let { accept(it.first) }
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
