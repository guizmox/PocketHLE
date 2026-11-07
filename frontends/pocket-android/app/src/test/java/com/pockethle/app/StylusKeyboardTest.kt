package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class StylusKeyboardTest {
    @Test fun defaultPAndReassignedClickAndDirectionsAreExcludedFromGrids() {
        val state=InputEditorState("{}")
        var bindings=InputBindings(LauncherConfig.default(),false)
        assertEquals(DeckInputPolicy.STYLUS_CLICK_COMMAND,bindings.keyboardName("P"))
        assertFalse(state.fixed("P",false,false))
        for(key in listOf("P","p","O","K","L","M")) assertTrue(bindings.stylusKeyboardReserved(key))
        assertTrue(state.assign("P",false,null,false))
        assertTrue(state.assign("J",false,"stylus_click",false))
        assertTrue(state.assign("O",false,"action",false))
        assertTrue(state.assign("I",false,"stylus:0",false))
        bindings=InputBindings(LauncherConfig.default().copy(originalJson=state.json.toString()),false)
        assertFalse(bindings.stylusKeyboardReserved("P"));assertFalse(bindings.stylusKeyboardReserved("O"))
        assertTrue(bindings.stylusKeyboardReserved("J"));assertTrue(bindings.stylusKeyboardReserved("I"))
        assertNull(bindings.keyboardName("P"));assertEquals(DeckInputPolicy.STYLUS_CLICK_COMMAND,bindings.keyboardName("J"))
        assertFalse(state.visibleButtons(true).contains("stylus_click"))
        assertFalse(state.targets(false,"J",true,false).contains("stylus_click"))
        assertFalse(state.targets(true,"South",false,false).contains("stylus_click"))
        assertNull(InputBindings(LauncherConfig.default(),true).keyboardName("P"))
    }
}
