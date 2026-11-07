package com.pockethle.app

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class DeckInputPolicyTest {
    private fun defaults() =
        JSONObject(
            javaClass.getResourceAsStream("/deck-input-defaults.json")!!.bufferedReader().readText()
        )

    @Test
    fun factoryMapsAndProfileOverridesMatchSharedDeckReference() {
        val reference = defaults()
        val expected = reference.getJSONObject("controller")
        assertTrue(InputBindings.defaultController().similar(expected))
        val codes = reference.getJSONObject("vk_codes")
        codes.keys().forEach { name ->
            assertEquals(codes.getInt(name), InputBindings.buttons[name]!!.second)
        }
        assertEquals("1", InputBindings.namedKey(android.view.KeyEvent.KEYCODE_1))
        assertEquals("2", InputBindings.namedKey(android.view.KeyEvent.KEYCODE_2))
        assertEquals("F12", InputBindings.namedKey(android.view.KeyEvent.KEYCODE_F12))
        val actualKeys = InputBindings.defaultKeyboard()
        val keys = reference.getJSONArray("keyboard")
        for (i in 0 until keys.length()) {
            val entry = keys.getJSONObject(i)
            val other =
                (0 until actualKeys.length())
                    .map { actualKeys.getJSONObject(it) }
                    .single { it.getString("button") == entry.getString("button") }
            assertTrue(other.getJSONArray("keys").similar(entry.getJSONArray("keys")))
        }
        for ((gizmondo, field) in listOf(false to "ppc_overrides", true to "gizmondo_overrides")) {
            val overrides = reference.getJSONObject(field)
            overrides.keys().forEach { name ->
                assertEquals(overrides.getString(name), DeckInputPolicy.guest(name, gizmondo))
            }
            val config =
                LauncherConfig.default()
                    .copy(originalJson = JSONObject().put("gamepad_bindings", expected).toString())
            val bindings = InputBindings(config, gizmondo)
            overrides.keys().forEach { name ->
                assertEquals(
                    InputBindings.buttons[overrides.getString(name)]!!.second,
                    bindings.controllerKey(name),
                )
            }
            val controls =
                org.json.JSONArray(
                    javaClass
                        .getResourceAsStream("/controller-layout.json")!!
                        .bufferedReader()
                        .readText()
                )
            for (i in 0 until controls.length()) {
                val name = controls.getJSONObject(i).getString("input")
                val host =
                    name in listOf("Select", "Start", "Misc1") || (!gizmondo && name == "Mode")
                val button = overrides.optString(name).ifEmpty { expected.optString(name) }
                assertEquals(
                    "$name / gizmondo=$gizmondo",
                    if (host) null else InputBindings.buttons[button]?.second,
                    bindings.controllerKey(name),
                )
            }
        }
    }

    @Test
    fun gizmondoHasNoTurboAndLegacyF3DisplaysBrightness() {
        val state = InputEditorState("{}")
        assertFalse(state.visibleButtons(true).contains("turbo"))
        assertTrue(state.visibleButtons(false).contains("turbo"))
        state.assign("T", false, "turbo")
        assertEquals("giz_piano3", state.assignment("T", false, true))
        assertEquals("turbo", state.assignment("T", false, false))
        assertTrue(state.assigned("giz_piano3", true, true, listOf("Select")))
        assertEquals(DeckInputPolicy.Action.BRIGHTNESS, DeckInputPolicy.hold("Select", true))
        state.reset(true, defaults())
        assertTrue(state.controller.similar(defaults().getJSONObject("controller")))
    }

    @Test
    fun longPressNeverAlsoExecutesTapAndPauseCancelsPendingActions() {
        val held = DeckHostTracker()
        assertTrue(held.down(1, "Back", 0, true))
        assertTrue(held.due(649, true).isEmpty())
        assertEquals(listOf(DeckInputPolicy.Action.BRIGHTNESS), held.due(650, true))
        assertNull(held.up(1, "Select", 800, true))
        held.down(2, "Select", 1000, false)
        assertEquals(DeckInputPolicy.Action.FULLSCREEN, held.up(2, "Back", 1200, false))
        held.down(1, "Start", 2000, false)
        held.clear()
        assertTrue(held.due(3000, false).isEmpty())
        assertNull(held.up(1, "Start", 3000, false))
    }
}
