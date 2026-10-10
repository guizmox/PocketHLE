package com.pockethle.app

import android.view.KeyEvent
import org.json.JSONArray
import org.json.JSONObject

/** Same stable button and host-control names as pocket-library / desktop SDL. */
internal class InputBindings(config: LauncherConfig) {
    val keyboard = JSONArray(config.keybindingsJson ?: "[]")
    val controller = JSONObject(config.originalJson).optJSONObject("gamepad_bindings") ?: defaultController()
    fun keyboardKey(event: KeyEvent): Int? {
        val name=keyName(event) ?: return null
        for(i in 0 until keyboard.length()) {
            val entry=keyboard.getJSONObject(i);val keys=entry.optJSONArray("keys") ?: continue
            for(j in 0 until keys.length()) if(canonical(keys.getString(j)).equals(canonical(name),true)) return buttons[entry.optString("button")]?.second
        }
        return null
    }
    fun controllerKey(name: String): Int? = buttons[controller.optString(name)]?.second
    companion object {
        val buttons=linkedMapOf(
            "dpad_up" to ("D-pad ↑" to 0x26),"dpad_down" to ("D-pad ↓" to 0x28),"dpad_left" to ("D-pad ←" to 0x25),"dpad_right" to ("D-pad →" to 0x27),
            "action" to ("Play ▶" to 0x0d),"button_a" to ("Stop ■" to 0x11),"button_b" to ("Forward ▶▶" to 0x20),"button_c" to ("Rewind ◀◀" to 0x10),
            "soft1" to ("L" to 0x09),"soft2" to ("R" to 0x1b),"giz_piano1" to ("Home" to 0x70),"giz_piano2" to ("Volume" to 0x71),"giz_piano3" to ("Brightness" to 0x72),"giz_piano4" to ("Geofence" to 0x73),"giz_piano5" to ("Power" to 0x7a),"turbo" to ("Turbo (PocketHLE)" to 0x72)
        )
        val controls=listOf("South","East","North","West","Start","Back","Guide","LeftTrigger","RightTrigger","LeftTrigger2","RightTrigger2","LeftStick","RightStick","DPadUp","DPadDown","DPadLeft","DPadRight","LeftStickUp","LeftStickDown","LeftStickLeft","LeftStickRight","RightStickUp","RightStickDown","RightStickLeft","RightStickRight")
        fun defaultController()=JSONObject().apply {
            listOf("South" to "action","North" to "button_a","East" to "button_b","West" to "button_c","LeftTrigger" to "soft1","RightTrigger" to "soft2","LeftTrigger2" to "turbo","Start" to "giz_piano1").forEach { (a,b)->put(a,b) }
            listOf("Up","Down","Left","Right").forEach { direction -> put("DPad$direction","dpad_${direction.lowercase()}");put("LeftStick$direction","dpad_${direction.lowercase()}") }
        }
        fun canonical(name: String)=if(name.startsWith("Arrow",true)) name.substring(5) else name
        fun keyName(event: KeyEvent): String? = when(event.keyCode) {
            in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> "F${event.keyCode-KeyEvent.KEYCODE_F1+1}"
            KeyEvent.KEYCODE_DPAD_UP -> "Up";KeyEvent.KEYCODE_DPAD_DOWN -> "Down";KeyEvent.KEYCODE_DPAD_LEFT -> "Left";KeyEvent.KEYCODE_DPAD_RIGHT -> "Right"
            KeyEvent.KEYCODE_ENTER -> "Enter";KeyEvent.KEYCODE_SPACE -> "Space";KeyEvent.KEYCODE_TAB -> "Tab";KeyEvent.KEYCODE_ESCAPE -> "Escape"
            KeyEvent.KEYCODE_DEL -> "Backspace";KeyEvent.KEYCODE_FORWARD_DEL -> "Delete"
            else -> event.getUnicodeChar(0).takeIf { it>32 && it<=126 }?.toChar()?.uppercaseChar()?.toString()
        }
        fun controllerName(key: Int): String? = when(key) {
            KeyEvent.KEYCODE_BUTTON_A->"South";KeyEvent.KEYCODE_BUTTON_B->"East";KeyEvent.KEYCODE_BUTTON_X->"West";KeyEvent.KEYCODE_BUTTON_Y->"North"
            KeyEvent.KEYCODE_BUTTON_START->"Start";KeyEvent.KEYCODE_BUTTON_SELECT->"Back";KeyEvent.KEYCODE_BUTTON_MODE->"Guide"
            KeyEvent.KEYCODE_BUTTON_L1->"LeftTrigger";KeyEvent.KEYCODE_BUTTON_R1->"RightTrigger";KeyEvent.KEYCODE_BUTTON_L2->"LeftTrigger2";KeyEvent.KEYCODE_BUTTON_R2->"RightTrigger2"
            KeyEvent.KEYCODE_BUTTON_THUMBL->"LeftStick";KeyEvent.KEYCODE_BUTTON_THUMBR->"RightStick"
            KeyEvent.KEYCODE_DPAD_UP->"DPadUp";KeyEvent.KEYCODE_DPAD_DOWN->"DPadDown";KeyEvent.KEYCODE_DPAD_LEFT->"DPadLeft";KeyEvent.KEYCODE_DPAD_RIGHT->"DPadRight"
            else->null
        }
    }
}
