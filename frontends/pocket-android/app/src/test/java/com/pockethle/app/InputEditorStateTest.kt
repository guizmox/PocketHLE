package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class InputEditorStateTest {
    private fun draft() =
        InputEditorState(
            """{"keybindings":[{"button":"action","keys":["Z"]}],"gamepad_bindings":{"South":"action","LeftStick":"soft1"},"untouched":"keep"}"""
        )

    @Test
    fun stylusPriorityAndDevicePreview() {
        val state = draft()
        assertEquals("stylus:0", state.assignment("O", false, false))
        assertNull(state.assignment("O", false, true))
        assertEquals("action", state.assignment("South", true, false))
        assertFalse(state.visibleButtons(false).contains("giz_piano1"))
        assertFalse(state.visibleButtons(true).contains("pocket_pc_app1"))
    }

    @Test
    fun movingInputClearsGuestStylusAndFullscreenConflicts() {
        val state = draft()
        state.json.put("gamepad_fullscreen", "LeftThumb")
        assertTrue(state.assign("LeftThumb", true, "stylus:1"))
        assertFalse(state.controller.has("LeftStick"))
        assertTrue(state.json.isNull("gamepad_fullscreen"))
        assertEquals("stylus:1", state.assignment("LeftStick", true, false))
        state.assign("O", false, "action")
        assertEquals("action", state.assignment("O", false, false))
        state.assign("O", false, null)
        assertNull(state.assignment("O", false, false))
        assertEquals("keep", state.json.getString("untouched"))
    }

    @Test
    fun screenshotKeyIsProtectedAndDraftDoesNotMutateSource() {
        val raw = draft().json.toString()
        val state = InputEditorState(raw)
        assertFalse(state.assign("F10", false, "action"))
        assertEquals("host", state.assignment("F10", false, false))
        assertEquals(raw, state.json.toString())
        state.assign("X", false, "button_a")
        assertFalse(JSONObject(raw).toString().contains("button_a"))
    }

    @Test
    fun desktopControllerNamesResolveInAndroidRuntime() {
        val config =
            LauncherConfig.default()
                .copy(
                    originalJson =
                        """{"gamepad_bindings":{"Select":"soft1","LeftThumb":"action"}}"""
                )
        val bindings = InputBindings(config)
        assertEquals(0x09, bindings.controllerKey("Back"))
        assertEquals(0x0d, bindings.controllerKey("LeftStick"))
        assertEquals("LeftThumb", InputBindings.controllerCanonical("LeftStick"))
    }

    @Test
    fun skinResourcesAreSharedAndKeyboardGeometryDoesNotOverlap() {
        val keyboard =
            JSONArray(
                javaClass.getResourceAsStream("/keyboard-layout.json")!!.bufferedReader().readText()
            )
        for (i in 0 until keyboard.length()) {
            val a = keyboard.getJSONObject(i).getJSONArray("rect")
            val x = a.getDouble(0)
            val y = a.getDouble(1)
            val w = a.getDouble(2)
            val h = a.getDouble(3)
            assertTrue(x >= 0 && y >= 0 && x + w <= 1000 && y + h <= 340)
            for (j in i + 1 until keyboard.length()) {
                val b = keyboard.getJSONObject(j).getJSONArray("rect")
                assertFalse(
                    x < b.getDouble(0) + b.getDouble(2) &&
                        x + w > b.getDouble(0) &&
                        y < b.getDouble(1) + b.getDouble(3) &&
                        y + h > b.getDouble(1)
                )
            }
        }
        val controller =
            JSONArray(
                javaClass
                    .getResourceAsStream("/controller-layout.json")!!
                    .bufferedReader()
                    .readText()
            )
        assertEquals(26, controller.length())
    }
}
