package com.pockethle.app

import android.view.KeyEvent
import org.json.JSONArray
import org.json.JSONObject

/** Same stable button and host-control names as pocket-library / desktop SDL. */
internal class InputBindings(config: LauncherConfig, private val gizmondo: Boolean? = null) {
    val keyboard = config.keybindingsJson?.let { JSONArray(it) } ?: defaultKeyboard()
    val controller =
        JSONObject(config.originalJson).optJSONObject("gamepad_bindings") ?: defaultController()
    val shortPress =
        JSONObject(config.originalJson).optJSONObject("short_press_bindings") ?: JSONObject()

    fun hasShort(input: String, pad: Boolean) =
        (if (pad) !DeckInputPolicy.reserved(input, gizmondo ?: false)
        else !DeckInputPolicy.reservedKey(input)) &&
            shortPress.has(InputGestures.bindingKey(gizmondo ?: false, pad, input))

    fun shortKey(input: String, pad: Boolean): Int? =
        if (
            if (pad) DeckInputPolicy.reserved(input, gizmondo ?: false)
            else DeckInputPolicy.reservedKey(input)
        )
            null
        else
            buttons[shortPress.optString(InputGestures.bindingKey(gizmondo ?: false, pad, input))]
                ?.second
                ?.takeUnless {
                    it == DeckInputPolicy.SCREENSHOT_COMMAND ||
                        (gizmondo == true && it == CenterTapPulse.COMMAND)
                }

    val longPress =
        JSONObject(config.originalJson).optJSONObject("long_press_bindings") ?: JSONObject()

    fun longKey(input: String, pad: Boolean): Int? {
        val giz = gizmondo ?: false
        if (
            (pad && DeckInputPolicy.reserved(input, giz)) ||
                (!pad && (DeckInputPolicy.reservedKey(input) || input == "Escape"))
        )
            return null
        return buttons[longPress.optString(InputGestures.bindingKey(giz, pad, input))]
            ?.second
            ?.takeUnless {
                it == DeckInputPolicy.SCREENSHOT_COMMAND || (giz && it == CenterTapPulse.COMMAND)
            }
    }

    fun keyboardName(name: String): Int? {
        if (DeckInputPolicy.reservedKey(name)) return null
        if (hasShort(name, false)) return shortKey(name, false)
        for (i in 0 until keyboard.length()) {
            val entry = keyboard.getJSONObject(i)
            val keys = entry.optJSONArray("keys") ?: continue
            for (j in 0 until keys.length()) if (
                canonical(keys.getString(j)).equals(canonical(name), true)
            )
                return buttons[
                        DeckInputPolicy.keyboardGuest(
                            name,
                            entry.optString("button"),
                            gizmondo ?: false,
                        )]
                    ?.second
        }
        return buttons[DeckInputPolicy.keyboardGuest(name, null, gizmondo ?: false)]?.second
    }

    fun keyboardKey(event: KeyEvent): Int? = keyName(event)?.let { keyboardName(it) }

    fun controllerKey(name: String): Int? {
        if (gizmondo != null) {
            if (DeckInputPolicy.reserved(name, gizmondo)) return null
            if (hasShort(name, true)) return shortKey(name, true)
            DeckInputPolicy.guest(name, gizmondo)?.let {
                return buttons[it]?.second
            }
        }
        return controllerAliases(name).firstNotNullOfOrNull {
            buttons[controller.optString(it)]?.second
        }
    }

    companion object {
        val buttons =
            linkedMapOf(
                "pocket_pc_app1" to ("Pocket PC P1" to 0xc1),
                "pocket_pc_app2" to ("Pocket PC P2" to 0xc2),
                "pocket_pc_app3" to ("Pocket PC P3" to 0xc3),
                "pocket_pc_app4" to ("Pocket PC P4" to 0xc4),
                "dpad_up" to ("D-pad ↑" to 0x26),
                "dpad_down" to ("D-pad ↓" to 0x28),
                "dpad_left" to ("D-pad ←" to 0x25),
                "dpad_right" to ("D-pad →" to 0x27),
                "action" to ("Action / Gizmondo Play" to 0x0d),
                "button_a" to ("Button A / Gizmondo Stop" to 0x11),
                "button_b" to ("Button B / Gizmondo Forward" to 0x20),
                "button_c" to ("Button C / Gizmondo Rewind" to 0x10),
                "soft1" to ("Soft key 1 / Gizmondo L" to 0x09),
                "soft2" to ("Soft key 2 / Gizmondo R" to 0x1b),
                "giz_piano1" to ("Home" to 0x70),
                "giz_piano2" to ("Volume" to 0x71),
                "giz_piano3" to ("Brightness" to 0x72),
                "giz_piano4" to ("Geofence" to 0x73),
                "giz_piano5" to ("Power" to 0x7a),
                "turbo" to ("Turbo (PocketHLE)" to 0x72),
                "center_tap" to ("Center tap" to CenterTapPulse.COMMAND),
                "rotate_screen" to ("Rotation" to DeckInputPolicy.ROTATION_COMMAND),
                "screenshot" to ("Screenshot" to DeckInputPolicy.SCREENSHOT_COMMAND),
            )

        fun defaultKeyboard() =
            JSONArray().apply {
                val defaults =
                    mapOf(
                        "dpad_up" to "Z",
                        "dpad_down" to "S",
                        "dpad_left" to "Q",
                        "dpad_right" to "D",
                        "action" to "Down",
                        "button_a" to "Up",
                        "button_b" to "Right",
                        "button_c" to "Left",
                        "soft1" to "1",
                        "soft2" to "2",
                        "turbo" to "T",
                        "giz_piano1" to "F1",
                        "giz_piano2" to "F2",
                        "giz_piano3" to "F3",
                        "giz_piano4" to "F4",
                        "giz_piano5" to "F5",
                    )
                buttons.keys.forEach { button ->
                    put(
                        JSONObject()
                            .put("button", button)
                            .put("keys", JSONArray().apply { defaults[button]?.let { put(it) } })
                    )
                }
            }

        fun defaultController() =
            JSONObject().apply {
                listOf(
                        "South" to "action",
                        "North" to "button_a",
                        "East" to "button_b",
                        "West" to "button_c",
                        "RightTrigger" to "giz_piano1",
                        "LeftTrigger2" to "soft1",
                        "RightTrigger2" to "soft2",
                        "Start" to "giz_piano2",
                    )
                    .forEach { (a, b) -> put(a, b) }
                listOf("Up", "Down", "Left", "Right").forEach { direction ->
                    put("DPad$direction", "dpad_${direction.lowercase()}")
                    put("LeftStick$direction", "dpad_${direction.lowercase()}")
                }
            }

        fun controllerCanonical(name: String) =
            when (name) {
                "Back" -> "Select"
                "Guide" -> "Mode"
                "LeftStick" -> "LeftThumb"
                "RightStick" -> "RightThumb"
                else -> name
            }

        fun controllerAliases(name: String): List<String> {
            val canonical = controllerCanonical(name)
            val legacy =
                when (canonical) {
                    "Select" -> "Back"
                    "Mode" -> "Guide"
                    "LeftThumb" -> "LeftStick"
                    "RightThumb" -> "RightStick"
                    else -> canonical
                }
            return listOf(legacy, canonical).distinct()
        }

        fun canonical(name: String): String =
            when (name) {
                "Minus" -> "-"
                "Plus" -> "+"
                "Semicolon" -> ";"
                "Colon" -> ":"
                "Backslash" -> "\\"
                "Pipe" -> "|"
                "Questionmark" -> "?"
                "Backtick" -> "`"
                "Equals" -> "="
                "OpenBracket" -> "["
                "CloseBracket" -> "]"
                "Comma" -> ","
                "Period" -> "."
                "Slash" -> "/"
                else -> if (name.startsWith("Arrow", true)) name.substring(5) else name
            }

        fun keyName(event: KeyEvent): String? = keyName(event.keyCode, event.getUnicodeChar(0))

        fun keyName(code: Int, unicode: Int): String? =
            namedKey(code)
                ?: unicode.takeIf { it > 32 && it <= 126 }?.toChar()?.uppercaseChar()?.toString()

        fun namedKey(code: Int): String? =
            when (code) {
                in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> (code - KeyEvent.KEYCODE_0).toString()
                in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> "F${code-KeyEvent.KEYCODE_F1+1}"
                KeyEvent.KEYCODE_DPAD_UP -> "Up"
                KeyEvent.KEYCODE_DPAD_DOWN -> "Down"
                KeyEvent.KEYCODE_DPAD_LEFT -> "Left"
                KeyEvent.KEYCODE_DPAD_RIGHT -> "Right"
                KeyEvent.KEYCODE_ENTER -> "Enter"
                KeyEvent.KEYCODE_SPACE -> "Space"
                KeyEvent.KEYCODE_TAB -> "Tab"
                KeyEvent.KEYCODE_ESCAPE -> "Escape"
                KeyEvent.KEYCODE_MOVE_HOME -> "Home"
                KeyEvent.KEYCODE_MOVE_END -> "End"
                KeyEvent.KEYCODE_PAGE_UP -> "PageUp"
                KeyEvent.KEYCODE_PAGE_DOWN -> "PageDown"
                KeyEvent.KEYCODE_INSERT -> "Insert"
                KeyEvent.KEYCODE_DEL -> "Backspace"
                KeyEvent.KEYCODE_FORWARD_DEL -> "Delete"
                else -> null
            }

        fun controllerName(key: Int): String? =
            when (key) {
                KeyEvent.KEYCODE_BUTTON_A -> "South"
                KeyEvent.KEYCODE_BUTTON_B -> "East"
                KeyEvent.KEYCODE_BUTTON_X -> "West"
                KeyEvent.KEYCODE_BUTTON_Y -> "North"
                KeyEvent.KEYCODE_BUTTON_START -> "Start"
                KeyEvent.KEYCODE_BUTTON_SELECT -> "Back"
                KeyEvent.KEYCODE_BUTTON_MODE -> "Guide"
                KeyEvent.KEYCODE_BUTTON_L1 -> "LeftTrigger"
                KeyEvent.KEYCODE_BUTTON_R1 -> "RightTrigger"
                KeyEvent.KEYCODE_BUTTON_L2 -> "LeftTrigger2"
                KeyEvent.KEYCODE_BUTTON_R2 -> "RightTrigger2"
                KeyEvent.KEYCODE_BUTTON_THUMBL -> "LeftStick"
                KeyEvent.KEYCODE_BUTTON_THUMBR -> "RightStick"
                KeyEvent.KEYCODE_DPAD_UP -> "DPadUp"
                KeyEvent.KEYCODE_DPAD_DOWN -> "DPadDown"
                KeyEvent.KEYCODE_DPAD_LEFT -> "DPadLeft"
                KeyEvent.KEYCODE_DPAD_RIGHT -> "DPadRight"
                else -> null
            }
    }
}
