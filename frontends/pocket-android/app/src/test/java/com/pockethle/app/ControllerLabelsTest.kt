package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class ControllerLabelsTest {
    @Test fun usesReportedCharactersWithoutAssumingXboxOrNintendo() {
        assertEquals("B", reportedControllerLabel("South", 'B'))
        assertEquals("A", reportedControllerLabel("East", 'A'))
        assertEquals("×", reportedControllerLabel("South", '×'))
    }
    @Test fun gridKeepsKeyboardAndVirtualTouchNamesSeparateFromControllerMarkings() {
        assertEquals("Key A", ControllerLabels.grid(GridInput("keyboard","A")))
        assertEquals("Touch ButtonA", ControllerLabels.grid(GridInput("controller","Touch:ButtonA")))
    }
    @Test fun missingCharactersUseExplicitPositionsInsteadOfInventedLetters() {
        for (missing in listOf(null, '\u0000', ' ', '\n')) {
            assertEquals("Bottom button", reportedControllerLabel("South", missing))
            assertEquals("Right button", reportedControllerLabel("East", missing))
        }
    }
}
