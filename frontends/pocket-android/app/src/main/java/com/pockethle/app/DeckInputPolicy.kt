package com.pockethle.app

/** Deck is the shared input reference. Policy is independent of Android events. */
internal object DeckInputPolicy {
    const val HOLD_MS = 650L

    enum class Action {
        FULLSCREEN,
        EXIT,
        BRIGHTNESS,
        NEXT_FILTER,
        RECORDING,
        SCREENSHOT,
    }

    fun tap(control: String): Action? =
        when (InputBindings.controllerCanonical(control)) {
            "Select" -> Action.FULLSCREEN
            "Start",
            "Misc1" -> Action.NEXT_FILTER
            else -> null
        }

    fun hold(control: String, gizmondo: Boolean): Action? =
        when (InputBindings.controllerCanonical(control)) {
            "Select" -> if (gizmondo) Action.BRIGHTNESS else Action.EXIT
            "Start",
            "Misc1" -> Action.RECORDING
            "Mode" -> if (gizmondo) null else Action.EXIT
            else -> null
        }

    fun reserved(control: String, gizmondo: Boolean) =
        tap(control) != null || hold(control, gizmondo) != null

    fun guest(control: String, gizmondo: Boolean): String? {
        val name = InputBindings.controllerCanonical(control)
        if (gizmondo) return if (name == "LeftTrigger") "giz_piano2" else null
        return when (name) {
            "LeftTrigger" -> "pocket_pc_app1"
            "RightTrigger" -> "pocket_pc_app2"
            "LeftTrigger2" -> "pocket_pc_app3"
            "RightTrigger2" -> "pocket_pc_app4"
            "LeftThumb" -> "soft2"
            "RightThumb" -> "action"
            "South" -> "button_a"
            "East" -> "button_b"
            "West" -> "button_c"
            "North" -> "soft1"
            else -> null
        }
    }

    fun reservedKey(name:String) = name.uppercase() in listOf("F8","F9","F10","F11","F12")

    fun key(name: String): Action? =
        when (name.uppercase()) {
            "F8" -> Action.EXIT
            "F9" -> Action.NEXT_FILTER
            "F10" -> Action.SCREENSHOT
            "F11" -> Action.RECORDING
            "F12" -> Action.FULLSCREEN
            else -> null
        }

    fun label(action: Action) =
        when (action) {
            Action.FULLSCREEN -> "Fullscreen"
            Action.EXIT -> "Return to library"
            Action.BRIGHTNESS -> "Brightness"
            Action.NEXT_FILTER -> "Next filter"
            Action.RECORDING -> "Recording (desktop only)"
            Action.SCREENSHOT -> "Screenshot"
        }

    fun short(action: Action) =
        when (action) {
            Action.EXIT -> "Exit"
            Action.NEXT_FILTER -> "Filter"
            Action.RECORDING -> "Record*"
            Action.SCREENSHOT -> "Photo"
            Action.FULLSCREEN -> "Full"
            Action.BRIGHTNESS -> "Brightness"
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
        if (!DeckInputPolicy.reserved(control, gizmondo)) return false
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
