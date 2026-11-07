package com.pockethle.app

/** Editor-only launch chords; release all launch buttons before capturing an assignment. */
internal class AssignmentShortcuts {
    private data class Input(val device: Int, val pad: Boolean, val name: String)

    private data class Launch(val device: Int, val pad: Boolean)

    private val held = mutableSetOf<Input>()
    private val ctrl = mutableSetOf<Int>()
    private var launch: Launch? = null
    val awaitingRelease: Boolean
        get() = launch != null

    fun key(
        device: Int,
        pad: Boolean,
        name: String,
        down: Boolean,
        repeat: Boolean,
        ctrlPressed: Boolean,
        controllerScreen: Boolean,
        active: Boolean,
        extraModifiers: Boolean = false,
    ): Boolean {
        val input = Input(device, pad, if (pad) InputBindings.controllerCanonical(name) else name)
        if (down) held.add(input) else held.remove(input)
        if (!pad) {
            if (ctrlPressed) ctrl.add(device) else ctrl.remove(device)
        }
        launch?.let {
            val waiting =
                if (it.pad)
                    held.any { key ->
                        key.device == it.device &&
                            key.pad &&
                            key.name in listOf("LeftThumb", "RightThumb")
                    }
                else it.device in ctrl || Input(it.device, false, "Enter") in held
            if (!waiting) launch = null
            return false
        }
        if (active || repeat || !down || pad != controllerScreen) return false
        val matches =
            if (pad)
                Input(device, true, "LeftThumb") in held &&
                    Input(device, true, "RightThumb") in held
            else input.name == "Enter" && ctrlPressed && !extraModifiers
        if (matches) {
            launch = Launch(device, pad)
            return true
        }
        return false
    }

    fun removeDevice(device: Int) {
        held.removeAll { it.device == device }
        ctrl.remove(device)
        if (launch?.device == device) launch = null
    }

    fun clear() {
        held.clear()
        ctrl.clear()
        launch = null
    }
}
