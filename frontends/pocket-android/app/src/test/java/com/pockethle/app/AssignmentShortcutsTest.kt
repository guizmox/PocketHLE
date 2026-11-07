package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class AssignmentShortcutsTest {
    @Test
    fun ctrlEnterWaitsForCtrlAndEnterAndIgnoresRepeatAndOtherScreens() {
        val s = AssignmentShortcuts()
        assertFalse(s.key(1, false, "Enter", true, false, true, true, false))
        s.clear()
        assertFalse(s.key(1, false, "Enter", true, true, true, false, false))
        assertFalse(s.key(1, false, "Enter", true, false, true, false, false, true))
        assertTrue(s.key(1, false, "Enter", true, false, true, false, false))
        assertTrue(s.awaitingRelease)
        assertFalse(s.key(1, false, "Enter", false, false, true, false, true))
        assertTrue(s.awaitingRelease)
        assertFalse(s.key(1, false, "Ctrl", false, false, false, false, true))
        assertFalse(s.awaitingRelease)
        assertFalse(s.key(1, false, "Enter", true, false, true, false, true))
    }

    @Test
    fun stickChordMustUseSameControllerAndWaitForBothReleases() {
        val s = AssignmentShortcuts()
        assertFalse(s.key(1, true, "LeftStick", true, false, false, true, false))
        assertFalse(s.key(2, true, "RightStick", true, false, false, true, false))
        assertTrue(s.key(1, true, "RightThumb", true, false, false, true, false))
        assertTrue(s.awaitingRelease)
        assertFalse(s.key(1, true, "LeftThumb", false, false, false, true, true))
        assertTrue(s.awaitingRelease)
        assertFalse(s.key(1, true, "RightThumb", false, false, false, true, true))
        assertFalse(s.awaitingRelease)
        assertFalse(s.key(1, true, "LeftThumb", true, false, false, true, true))
    }

    @Test
    fun focusLossAndDisconnectClearPendingShortcut() {
        val s = AssignmentShortcuts()
        s.key(1, false, "Enter", true, false, true, false, false)
        s.removeDevice(2)
        assertTrue(s.awaitingRelease)
        s.removeDevice(1)
        assertFalse(s.awaitingRelease)
        s.key(3, true, "LeftThumb", true, false, false, true, false)
        assertTrue(s.key(3, true, "RightThumb", true, false, false, true, false))
        s.clear()
        assertFalse(s.awaitingRelease)
        assertFalse(s.key(3, true, "RightThumb", true, false, false, true, false))
    }
}
