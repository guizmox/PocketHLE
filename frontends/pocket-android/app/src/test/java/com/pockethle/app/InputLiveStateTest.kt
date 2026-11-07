package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class InputLiveStateTest {
    @Test
    fun simultaneousKeysAndDevicesReleaseIndependently() {
        val live = InputLiveState()
        live.key(1, false, 1, "Z", true)
        live.key(2, false, 2, "Z", true)
        live.key(1, false, 3, "Space", true)
        live.key(1, false, 1, "Z", false)
        assertTrue(live.isDown("Z", false))
        assertTrue(live.isDown("Space", false))
        live.removeDevice(2)
        assertFalse(live.isDown("Z", false))
        live.clear()
        assertFalse(live.isDown("Space", false))
    }

    @Test
    fun axisHysteresisAndKeyHatOverlapAvoidStickyHighlights() {
        val live = InputLiveState()
        live.axis(1, 0, .3f, "LeftStickLeft", "LeftStickRight")
        assertTrue(live.isDown("LeftStickRight", true))
        live.axis(1, 0, .2f, "LeftStickLeft", "LeftStickRight")
        assertTrue(live.isDown("LeftStickRight", true))
        live.axis(1, 0, .1f, "LeftStickLeft", "LeftStickRight")
        assertFalse(live.isDown("LeftStickRight", true))
        live.key(1, true, 10, "DPadUp", true)
        live.axis(1, 1, -1f, "DPadUp", "DPadDown")
        live.axis(1, 1, 0f, "DPadUp", "DPadDown")
        assertTrue(live.isDown("DPadUp", true))
        live.key(1, true, 10, "DPadUp", false)
        assertFalse(live.isDown("DPadUp", true))
        live.axis(1, 0, Float.NaN, "LeftStickLeft", "LeftStickRight")
        assertFalse(live.isDown("LeftStickRight", true))
    }

    @Test
    fun controllerAliasesAndKeyboardModifiersShareLogicalHighlight() {
        val live = InputLiveState()
        live.key(1, true, 1, "Back", true)
        assertTrue(live.isDown("Select", true))
        live.key(1, false, 2, "Shift", true)
        live.key(1, false, 3, "Shift", true)
        live.key(1, false, 2, "Shift", false)
        assertTrue(live.isDown("Shift", false))
        live.removeDevice(1)
        assertFalse(live.isDown("Select", true))
        assertFalse(live.isDown("Shift", false))
    }
}
