package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class CenterTapPulseTest {
    @Test
    fun importedDesktopScreenshotMappingsStayUnavailableOnAndroid() {
        val json =
            """{"short_press_bindings":{"pocketpc:keyboard:J":"screenshot"},"long_press_bindings":{"pocketpc:keyboard:J":"screenshot"}}"""
        val state = InputEditorState(json)
        val bindings = InputBindings(LauncherConfig.default().copy(originalJson = json), false)
        assertNull(state.assignment("J", false, false))
        assertNull(state.longTarget("J", false, false))
        assertNull(bindings.shortKey("J", false))
        assertNull(bindings.longKey("J", false))
    }

    @Test
    fun pulseUsesLogicalCenterAndReleasesOnceWithoutRepeating() {
        val tap = CenterTapPulse()
        assertEquals(120 to 160, tap.press(100, 240, 320))
        assertNull(tap.press(110, 640, 480))
        assertNull(tap.releaseDue(159))
        assertEquals(120 to 160, tap.releaseDue(160))
        assertNull(tap.releaseDue(200))
        assertEquals(320 to 240, tap.press(200, 640, 480))
        assertEquals(320 to 240, tap.cancel())
        assertNull(tap.cancel())
        assertFalse(tap.active)
        assertNull(tap.press(0, 0, 320))
    }

    @Test
    fun commandDefaultsAndAssignmentArePocketPcOnly() {
        val state = InputEditorState("{}")
        val ppc = InputBindings(LauncherConfig.default(), false)
        val giz = InputBindings(LauncherConfig.default(), true)
        assertEquals(CenterTapPulse.COMMAND, ppc.keyboardName("Tab"))
        assertEquals(CenterTapPulse.COMMAND, ppc.controllerKey("Mode"))
        assertNull(giz.keyboardName("Tab"))
        assertNull(giz.controllerKey("Mode"))
        assertEquals(DeckInputPolicy.ROTATION_COMMAND, ppc.controllerKey("Misc1"))
        assertEquals(DeckInputPolicy.ROTATION_COMMAND, giz.controllerKey("Misc1"))
        assertFalse(state.visibleButtons(false).contains("screenshot"))
        assertFalse(state.assign("J", false, "screenshot", false))
        assertTrue(state.assign("Misc1", true, null, false))
        assertNull(state.assignment("Misc1", true, false))
        assertEquals(DeckInputPolicy.Action.ROTATION, DeckInputPolicy.key("F7"))
        assertTrue(state.fixed("F7", false, false))
        assertFalse(state.assign("F7", false, "action", false))
        assertTrue(state.visibleButtons(false).contains("center_tap"))
        assertFalse(state.visibleButtons(true).contains("center_tap"))
        assertEquals("center_tap", state.assignment("Tab", false, false))
        assertEquals("center_tap", state.assignment("Mode", true, false))
        assertTrue(state.assign("J", false, "center_tap", false))
        assertEquals("center_tap", state.assignment("J", false, false))
        assertTrue(state.assign("Tab", false, "soft1", false))
        assertEquals("soft1", state.assignment("Tab", false, false))
        assertFalse(state.assign("J", false, "center_tap", true))
    }
}
