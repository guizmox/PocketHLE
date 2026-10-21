package com.pockethle.app
import org.junit.Assert.*
import org.junit.Test
class TouchRoutingTest {
    @Test fun buttonHeldThenScreenStillReceivesItsOwnDownAndUp() {
        assertEquals(0,routedTouchAction(0,7,listOf(7)))
        assertEquals(2,routedTouchAction(5,11,listOf(7)))
        assertEquals(0,routedTouchAction(5,11,listOf(11)))
        assertEquals(1,routedTouchAction(6,11,listOf(11)))
        assertEquals(2,routedTouchAction(6,11,listOf(7)))
        assertEquals(1,routedTouchAction(1,7,listOf(7)))
        assertEquals(0,routedTouchAction(0,3,listOf(3)))
    }
    @Test fun screenHeldThenButtonDoesNotStealOrReleaseStylus() {
        assertEquals(0,routedTouchAction(0,3,listOf(3)))
        assertEquals(2,routedTouchAction(5,9,listOf(3)))
        assertEquals(0,routedTouchAction(5,9,listOf(9)))
        assertEquals(1,routedTouchAction(6,9,listOf(9)))
        assertEquals(2,routedTouchAction(6,9,listOf(3)))
    }
    @Test fun pointerIndicesAreLocalToEachTargetAndCancelReachesBoth() {
        assertEquals(5 or (1 shl 8),routedTouchAction(5,9,listOf(3,9)))
        assertEquals(6,routedTouchAction(6,3,listOf(3,9)))
        assertEquals(3,routedTouchAction(3,3,listOf(3)))
        assertEquals(3,routedTouchAction(3,3,listOf(9)))
        assertNull(routedTouchAction(2,3,emptyList()))
    }
}
