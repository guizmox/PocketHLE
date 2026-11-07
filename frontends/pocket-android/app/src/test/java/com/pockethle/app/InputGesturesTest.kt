package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class InputGesturesTest {
    @Test
    fun shortOnlyOverridesAreImmediateAndReferenceCounted() {
        val t = InputGestures()
        assertEquals(listOf(1 to true), t.immediateEdge("a", true, 1))
        assertTrue(t.immediateEdge("a", true, 1).isEmpty())
        assertTrue(t.immediateEdge("b", true, 1).isEmpty())
        assertTrue(t.immediateEdge("a", false, null).isEmpty())
        assertEquals(listOf(1 to false), t.cancelPrefix("b"))
        assertTrue(t.immediateEdge("u", true, null).isEmpty())
        assertTrue(t.immediateEdge("u", false, null).isEmpty())
    }

    @Test
    fun customizedFaceOverridesDeckReferenceWithoutChangingOtherProfile() {
        val s = InputEditorState("{}")
        assertTrue(s.assign("South", true, "soft1", false))
        assertEquals("soft1", s.assignment("South", true, false))
        val config = LauncherConfig(originalJson = s.json.toString())
        assertEquals(9, InputBindings(config, false).controllerKey("South"))
        assertEquals(13, InputBindings(config, true).controllerKey("South"))
        assertTrue(s.assignLong("South", true, "action", false))
        assertEquals(
            13,
            InputBindings(LauncherConfig(originalJson = s.json.toString()), false)
                .longKey("South", true),
        )
    }

    @Test
    fun shortWaitsForReleaseAndPulses() {
        val t = InputGestures()
        assertTrue(t.edge("k", true, 0, 1, 2).isEmpty())
        assertEquals(listOf(1 to true), t.edge("k", false, 649, 1, 2))
        assertTrue(t.tick(728).isEmpty())
        assertEquals(listOf(1 to false), t.tick(729))
    }

    @Test
    fun longSuppressesShortAndIgnoresRepeat() {
        val t = InputGestures()
        t.edge("k", true, 0, 1, 2)
        t.edge("k", true, 600, 1, 2)
        assertEquals(listOf(2 to true), t.tick(650))
        assertTrue(t.tick(900).isEmpty())
        assertEquals(listOf(2 to false), t.edge("k", false, 1000, 1, 2))
    }

    @Test
    fun lateReleaseStillProducesObservableLongAction() {
        val t = InputGestures()
        t.edge("k", true, 0, 1, 2)
        assertEquals(listOf(2 to true), t.edge("k", false, 650, 1, 2))
        assertEquals(listOf(2 to false), t.tick(730))
    }

    @Test
    fun independentDevicesAndCancellationDoNotFireShort() {
        val t = InputGestures()
        t.edge("pad:1:a", true, 0, 1, 2)
        t.edge("pad:2:a", true, 0, 1, 2)
        assertEquals(listOf(2 to true), t.tick(650))
        assertTrue(t.cancelPrefix("pad:1:").isEmpty())
        assertEquals(listOf(2 to false), t.cancelPrefix("pad:2:"))
        t.clear()
        assertFalse(t.active())
        t.edge("k", true, 0, 1, 2)
        t.clear()
        assertTrue(t.tick(1000).isEmpty())
    }

    @Test
    fun longAssignmentRoundTripsPerProfileAndDoesNotReplaceShort() {
        val s = InputEditorState("{}")
        assertTrue(s.assign("X", false, "button_b", false))
        assertTrue(s.assignLong("X", false, "action", false))
        assertEquals("button_b", s.assignment("X", false, false))
        assertNull(s.longTarget("X", false, true))
        val restored = InputEditorState(s.json.toString())
        assertEquals("action", restored.longTarget("X", false, false))
        val bindings =
            InputBindings(
                LauncherConfig(
                    keybindingsJson = restored.keyboard.toString(),
                    originalJson = restored.json.toString(),
                ),
                false,
            )
        assertEquals(0x0d, bindings.longKey("X", false))
        assertEquals(0x20, bindings.keyboardName("X"))
        assertFalse(s.assignLong("F10", false, "action", false))
        assertFalse(s.assignLong("Start", true, "action", false))
        assertTrue(s.assignLong("O", false, "action", false))
        assertNull(s.assignment("O", false, false))
    }

    @Test
    fun namesAndResetMatchAcrossFrontends() {
        assertEquals("pocketpc:keyboard:-", InputGestures.bindingKey(false, false, "Minus"))
        assertEquals(
            "gizmondo:controller:LeftThumb",
            InputGestures.bindingKey(true, true, "LeftStick"),
        )
        val s = InputEditorState("{}")
        s.assignLong("X", false, "action", false)
        s.assignLong("X", false, "button_b", true)
        s.assignLong("LeftThumb", true, "action", true)
        s.reset(false, JSONObject().put("keyboard", InputBindings.defaultKeyboard()))
        assertNull(s.longTarget("X", false, false))
        assertNull(s.longTarget("X", false, true))
        assertEquals("action", s.longTarget("LeftThumb", true, true))
    }
}
