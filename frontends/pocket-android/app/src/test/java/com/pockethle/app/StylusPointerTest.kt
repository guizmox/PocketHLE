package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class StylusPointerTest {
    @Test
    fun doubleClickNeverTapsAndSingleClickOnlyTapsInPointerMode() {
        val p = StylusPointer()
        p.advance(0 to 0, 0f, 240, 320)
        p.button(true, 0)
        p.button(false, 10)
        assertNull(p.takeTap(310))
        p.button(true, 400)
        p.button(false, 410)
        p.button(true, 500)
        assertNull(p.takeTap(800))
        assertTrue(p.button(false, 810))
        assertTrue(p.pointer)
        p.button(true, 900)
        p.button(false, 910)
        assertNull(p.takeTap(1109))
        assertEquals(120 to 160, p.takeTap(1110))
        assertNull(p.takeTap(1111))
        p.button(true, 1300)
        p.button(false, 1310)
        p.button(true, 1400)
        p.button(false, 1410)
        assertFalse(p.pointer)
        assertNull(p.takeTap(1800))
    }

    @Test
    fun clampingAndCancellation() {
        val p = StylusPointer()
        p.button(true, 0)
        p.button(false, 10)
        p.button(true, 100)
        p.button(false, 110)
        p.advance(1200 to -1200, 5f, 40, 40)
        assertEquals(39 to 0, p.position())
        p.button(true, 200)
        p.button(false, 210)
        p.cancelClick()
        assertNull(p.takeTap(1000))
        p.advance(0 to 0, 0f, 10, 10)
        assertEquals(9 to 0, p.position())
    }

    @Test
    fun allImmutableInputsRejectGridAssignmentAndIgnoreSavedBindings() {
        val grid = StylusGridConfig()
        for (name in listOf("F7", "F8", "F9", "F10", "F11", "F12")) {
            val input = GridInput("keyboard", name)
            assertNotNull(grid.bind(0, input, null))
            assertNotNull(input.conflict(null))
        }
        for (name in
            listOf(
                "Select",
                "Start",
                "RightThumb",
                "RightStickUp",
                "RightStickDown",
                "RightStickLeft",
                "RightStickRight",
            )) {
            val input = GridInput("controller", name)
            assertNotNull(grid.bind(0, input, null))
            val raw =
                org.json
                    .JSONObject()
                    .put("columns", 4)
                    .put("rows", 5)
                    .put(
                        "bindings",
                        org.json
                            .JSONArray()
                            .put(
                                org.json
                                    .JSONObject()
                                    .put("cell", 0)
                                    .put(
                                        "input",
                                        org.json
                                            .JSONObject()
                                            .put("source", "controller")
                                            .put("name", name),
                                    )
                            ),
                    )
            assertNull(StylusGridConfig.parse(raw.toString()).cellFor(input, null))
            val state = InputEditorState("{}")
            assertFalse(state.assign(name, true, "action", false))
            assertFalse(state.assignLong(name, true, "action", false))
        }
    }
}
