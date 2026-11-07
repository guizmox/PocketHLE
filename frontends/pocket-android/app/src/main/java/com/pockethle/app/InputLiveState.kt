package com.pockethle.app

/** Physical sources are tracked separately: releasing a hat must not release a key. */
internal class InputLiveState {
    private data class Source(val device: Int, val pad: Boolean, val channel: String)

    private val held = mutableMapOf<Source, String>()

    private fun name(input: String, pad: Boolean) =
        if (pad) InputBindings.controllerCanonical(input)
        else InputBindings.canonical(input).uppercase()

    fun key(device: Int, pad: Boolean, code: Int, input: String, down: Boolean) {
        val source = Source(device, pad, "key:$code")
        if (down) held[source] = name(input, pad) else held.remove(source)
    }

    fun axis(
        device: Int,
        axis: Int,
        value: Float,
        negative: String?,
        positive: String,
        on: Float = .25f,
        off: Float = .15f,
    ) {
        fun direction(suffix: String, input: String?, strength: Float) {
            val source = Source(device, true, "axis:$axis:$suffix")
            if (
                input != null &&
                    strength.isFinite() &&
                    strength >= if (held.containsKey(source)) off else on
            )
                held[source] = name(input, true)
            else held.remove(source)
        }
        direction("-", negative, -value)
        direction("+", positive, value)
    }

    fun isDown(input: String, pad: Boolean) =
        held.any { (source, value) -> source.pad == pad && value == name(input, pad) }

    fun removeDevice(device: Int) {
        held.keys.removeAll { it.device == device }
    }

    fun clear() = held.clear()
}
