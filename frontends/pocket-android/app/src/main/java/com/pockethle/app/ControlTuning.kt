package com.pockethle.app

internal enum class ControlDeadzone(val radius: Float, private val press: Float, private val release: Float) {
    SMALL(0.16f, 0.35f, 0.20f), MEDIUM(0.305f, 0.60f, 0.35f), LARGE(0.45f, 0.75f, 0.50f);
    fun active(value: Float, held: Boolean) = value >= if (held) release else press
    companion object { fun from(value: String) = when(value) { "small" -> SMALL; "medium" -> MEDIUM; else -> LARGE } }
}
