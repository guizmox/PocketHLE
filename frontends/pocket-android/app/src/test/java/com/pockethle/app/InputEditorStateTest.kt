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
    fun keyboardDefaultsMatchProfileAndNumberNamesIgnoreShiftCharacters() {
        val state = InputEditorState("{}")
        val ppc = InputBindings(LauncherConfig.default(), false)
        val giz = InputBindings(LauncherConfig.default(), true)
        for (i in 1..4) {
            assertEquals("pocket_pc_app$i", state.assignment("$i", false, false))
            assertEquals(0xc0 + i, ppc.keyboardName("$i"))
        }
        for (key in listOf("F1", "F2", "F3", "F4", "F5")) {
            assertNull(state.assignment(key, false, false))
            assertNull(ppc.keyboardName(key))
        }
        assertNull(state.assignment("T", false, true))
        assertNull(giz.keyboardName("T"))
        assertEquals(0x72, giz.keyboardName("F3"))
        for (i in 1..9) for (character in listOf('&'.code, '!'.code, '1'.code)) {
            assertEquals(
                "$i",
                InputBindings.keyName(android.view.KeyEvent.KEYCODE_0 + i, character),
            )
        }
        assertTrue(state.assign("1", false, "action", false))
        assertEquals("action", state.assignment("1", false, false))
    }

    @Test
    fun gizmondoLabelsAndDefaultExtraAreProfileSpecific() {
        val state = InputEditorState("{}")
        for ((id, label) in
            mapOf(
                "action" to "Play",
                "button_a" to "Stop",
                "button_b" to "Forward",
                "button_c" to "Rewind",
                "soft1" to "L1",
                "soft2" to "R1",
            )) {
            assertEquals(label, state.label(id, true))
            assertEquals(label, state.short(id, true))
        }
        assertEquals("rotate_screen", state.assignment("Misc1", true, true))
        assertEquals("rotate_screen", state.assignment("Misc1", true, false))
        assertFalse(state.assigned("giz_piano3", true, true, listOf("Select")))
    }

    @Test
    fun guideStartsUnassignedAndCanBeAssignedIncludingHold() {
        for (giz in listOf(false, true)) {
            val state = draft()
            assertFalse(state.fixed("Mode", true, giz))
            assertEquals(if (giz) null else "center_tap", state.assignment("Mode", true, giz))
            assertEquals(if (giz) "—" else "Tap", state.shortFor("Mode", true, giz))
            assertTrue(state.assignLong("Mode", true, "action", giz))
            assertEquals("action", state.longTarget("Mode", true, giz))
            assertEquals(if (giz) null else "center_tap", state.assignment("Mode", true, giz))
            assertTrue(state.assign("Mode", true, "button_a", giz))
            assertEquals("button_a", state.assignment("Mode", true, giz))
            val runtime =
                InputBindings(
                    LauncherConfig.default().copy(originalJson = state.json.toString()),
                    giz,
                )
            assertEquals(0x11, runtime.controllerKey("Mode"))
            assertEquals(0x0d, runtime.longKey("Mode", true))
            assertTrue(state.assign("Mode", true, null, giz))
            assertNull(state.assignment("Mode", true, giz))
        }
    }

    @Test
    fun pocketPcLabelsUseGuestKeyNames() {
        val state = draft()
        for ((id, label) in
            mapOf(
                "action" to "ACTION",
                "soft1" to "TAB",
                "soft2" to "ESC",
                "turbo" to "TURBO",
                "button_a" to "A",
                "button_b" to "B",
                "button_c" to "C",
                "pocket_pc_app1" to "P1",
                "pocket_pc_app2" to "P2",
                "pocket_pc_app3" to "P3",
                "pocket_pc_app4" to "P4",
            )) {
            assertEquals(label, state.label(id, false))
            assertEquals(
                mapOf("action" to "Act", "soft1" to "Tab", "soft2" to "Esc")[id] ?: label,
                state.short(id, false),
            )
        }
        assertTrue(state.visibleButtons(false).containsAll(listOf("soft1", "soft2")))
        assertTrue(state.assign("Misc1", true, "soft2", false))
        assertEquals("soft2", state.assignment("Misc1", true, false))
    }

    @Test
    fun controllerFunctionButtonsAreImmutableIncludingLegacyOverrides() {
        for (giz in listOf(false, true)) for (name in listOf("Select", "Start")) {
            val key = InputGestures.bindingKey(giz, true, name)
            val raw =
                JSONObject()
                    .put("short_press_bindings", JSONObject().put(key, "action"))
                    .put("long_press_bindings", JSONObject().put(key, "button_a"))
            val state = InputEditorState(raw.toString())
            assertTrue(state.fixed(name, true, giz))
            assertFalse(state.assign(name, true, "action", giz))
            assertFalse(state.assignLong(name, true, "action", giz))
            assertEquals("host", state.assignment(name, true, giz))
            assertNull(state.longTarget(name, true, giz))
            val bindings =
                InputBindings(LauncherConfig.default().copy(originalJson = raw.toString()), giz)
            assertFalse(bindings.hasShort(name, true))
            assertNull(bindings.shortKey(name, true))
            assertNull(bindings.longKey(name, true))
        }
    }

    @Test
    fun functionKeysAreImmutableEvenWithLegacyGuestAndLongOverrides() {
        for (name in listOf("F7", "F8", "F9", "F10", "F11", "F12")) {
            val key = InputGestures.bindingKey(false, false, name)
            val raw =
                JSONObject()
                    .put(
                        "keybindings",
                        JSONArray()
                            .put(
                                JSONObject()
                                    .put("button", "action")
                                    .put("keys", JSONArray().put(name))
                            ),
                    )
                    .put("short_press_bindings", JSONObject().put(key, "action"))
                    .put("long_press_bindings", JSONObject().put(key, "button_a"))
            val state = InputEditorState(raw.toString())
            assertFalse(state.assign(name, false, "action", false))
            assertFalse(state.assignLong(name, false, "action", false))
            assertEquals("host", state.assignment(name, false, false))
            assertNull(state.longTarget(name, false, false))
            val input =
                InputBindings(
                    LauncherConfig.default()
                        .copy(
                            originalJson = raw.toString(),
                            keybindingsJson = raw.getJSONArray("keybindings").toString(),
                        ),
                    false,
                )
            assertNull(input.keyboardName(name))
            assertNull(input.longKey(name, false))
            assertNull(input.shortKey(name, false))
        }
        assertEquals(DeckInputPolicy.Action.EXIT, DeckInputPolicy.key("F8"))
        assertEquals(DeckInputPolicy.Action.NEXT_FILTER, DeckInputPolicy.key("F9"))
        assertEquals(DeckInputPolicy.Action.FULLSCREEN, DeckInputPolicy.key("F12"))
    }

    @Test
    fun assistantKeepsSupportedStylusAssignments() {
        val state = draft()
        assertTrue(state.targets(false, "O", false, false).contains("stylus:0"))
        assertTrue(state.targets(true, "RightStickUp", false, false).contains("stylus:0"))
        assertFalse(
            state.targets(true, "South", false, false).any { it?.startsWith("stylus:") == true }
        )
        assertFalse(
            state.targets(false, "O", true, false).any { it?.startsWith("stylus:") == true }
        )
        assertFalse(
            state.targets(false, "O", false, true).any { it?.startsWith("stylus:") == true }
        )
    }

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
        state.json.put("gamepad_fullscreen", "RightStickUp")
        assertTrue(state.assign("RightStickUp", true, "stylus:1"))
        assertFalse(state.controller.has("RightStickUp"))
        assertTrue(state.json.isNull("gamepad_fullscreen"))
        assertEquals("stylus:1", state.assignment("RightStickUp", true, false))
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
