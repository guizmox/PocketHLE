package com.pockethle.app

/** Deck is the shared input reference. Policy is independent of Android events. */
internal object DeckInputPolicy {
    const val STYLUS_CLICK_COMMAND = 0x1003
    const val ROTATION_COMMAND = 0x1001
    const val SCREENSHOT_COMMAND = 0x1002 // Portable config tag; unsupported on Android.
    const val HOLD_MS = 650L

    enum class Action {
        ROTATION,
        FULLSCREEN,
        EXIT,
        NEXT_FILTER,
    }

    fun tap(control: String): Action? =
        when (InputBindings.controllerCanonical(control)) {
            "Select" -> Action.FULLSCREEN
            "Start" -> Action.NEXT_FILTER
            else -> null
        }

    fun hold(control: String, gizmondo: Boolean): Action? =
        when (InputBindings.controllerCanonical(control)) {
            "Select" -> Action.EXIT
            else -> null
        }

    fun reserved(control: String, gizmondo: Boolean) =
        InputBindings.controllerCanonical(control) in listOf("Select", "Start") || stylusReserved(control,gizmondo)

    fun stylusReserved(control: String, gizmondo: Boolean) = !gizmondo &&
        InputBindings.controllerCanonical(control) in listOf("RightThumb","RightStickUp","RightStickDown","RightStickLeft","RightStickRight")

    fun guest(control: String, gizmondo: Boolean): String? {
        val name = InputBindings.controllerCanonical(control)
        if (gizmondo)
            return when (name) {
                "LeftTrigger" -> "giz_piano2"
                "Misc1" -> "rotate_screen"
                else -> null
            }
        return when (name) {
            "Misc1" -> "rotate_screen"
            "Mode" -> "soft1"
            "LeftTrigger" -> "pocket_pc_app1"
            "RightTrigger" -> "pocket_pc_app2"
            "LeftTrigger2" -> "pocket_pc_app3"
            "RightTrigger2" -> "pocket_pc_app4"
            "LeftThumb" -> "soft2"
            "RightThumb" -> null
            "South" -> "action"
            "East" -> "button_b"
            "West" -> "button_c"
            "North" -> "button_a"
            else -> null
        }
    }

    fun keyboardGuest(name: String, current: String?, gizmondo: Boolean): String? {
        if (current == "screenshot") return null
        if (gizmondo)
            return current?.takeUnless {
                it == "turbo" || it == "stylus_click" || it == "center_tap" || it.startsWith("pocket_pc_app")
            }
        if (current?.startsWith("giz_piano") == true) return null
        return when {
            name.equals("P",true) && current in listOf(null,"stylus_click") -> "stylus_click"
            name == "Tab" && current in listOf(null, "center_tap") -> "center_tap"
            name == "1" && current in listOf(null, "soft1", "pocket_pc_app1") -> "pocket_pc_app1"
            name == "2" && current in listOf(null, "soft2", "pocket_pc_app2") -> "pocket_pc_app2"
            name == "3" && current in listOf(null, "pocket_pc_app3") -> "pocket_pc_app3"
            name == "4" && current in listOf(null, "pocket_pc_app4") -> "pocket_pc_app4"
            else -> current
        }
    }

    fun reservedKey(name: String) =
        name.uppercase() in listOf("F7", "F8", "F9", "F10", "F11", "F12")

    fun key(name: String): Action? =
        when (name.uppercase()) {
            "F7" -> Action.ROTATION
            "F8" -> Action.EXIT
            "F9" -> Action.NEXT_FILTER
            "F12" -> Action.FULLSCREEN
            else -> null
        }

    fun label(action: Action) =
        when (action) {
            Action.ROTATION -> "Rotation"
            Action.FULLSCREEN -> "Fullscreen"
            Action.EXIT -> "Return to library"
            Action.NEXT_FILTER -> "Next filter"
        }

    fun short(action: Action) =
        when (action) {
            Action.EXIT -> "Exit"
            Action.NEXT_FILTER -> "Filter"
            Action.ROTATION -> "Rotate"
            Action.FULLSCREEN -> "Full"
        }

    val filters =
        listOf(
            "none",
            "reconstruction",
            "smaa",
            "smaa_soft",
            "xbrz",
            "bicubic",
            "lanczos",
            "bilinear",
            "nearest",
        )
}

/** One action per press; a long press never also fires the short action. */
internal class DeckHostTracker {
    private data class Press(val control: String, val at: Long, var fired: Boolean = false)

    private val held = mutableMapOf<Pair<Int, String>, Press>()

    fun down(device: Int, control: String, now: Long, gizmondo: Boolean): Boolean {
        if (DeckInputPolicy.tap(control) == null && DeckInputPolicy.hold(control, gizmondo) == null)
            return false
        val name = InputBindings.controllerCanonical(control)
        held.getOrPut(device to name) { Press(name, now) }
        return true
    }

    fun up(device: Int, control: String, now: Long, gizmondo: Boolean): DeckInputPolicy.Action? {
        val press = held.remove(device to InputBindings.controllerCanonical(control)) ?: return null
        if (press.fired) return null
        return if (now - press.at >= DeckInputPolicy.HOLD_MS)
            DeckInputPolicy.hold(press.control, gizmondo)
        else DeckInputPolicy.tap(press.control)
    }

    fun due(now: Long, gizmondo: Boolean): List<DeckInputPolicy.Action> =
        held.values.mapNotNull { press ->
            if (!press.fired && now - press.at >= DeckInputPolicy.HOLD_MS) {
                press.fired = true
                DeckInputPolicy.hold(press.control, gizmondo)
            } else null
        }

    fun clear() = held.clear()
}
