package com.pockethle.app

import org.junit.Assert.*
import org.junit.Test

class PocketPcButtonOrderTest {
    private val labels=listOf("A", "P2", "Action", "P1", "P4", "C", "P3", "B")
    @Test fun disabledKeepsExistingLayout() {
        assertEquals(labels, pocketPcButtonOrder(false,false).map { labels[it] })
    }
    @Test fun enabledMatchesRequestedControllerPositions() {
        val slots=pocketPcButtonOrder(true,true).map { labels[it] }
        assertEquals(listOf("B", "C", "Action", "A"), listOf(slots[3],slots[1],slots[6],slots[4]))
        assertEquals(listOf("P3", "P2", "P4", "P1"), listOf(slots[0],slots[2],slots[7],slots[5]))
        assertEquals(labels.toSet(), slots.toSet())
    }
    @Test fun enabledMatchesRequestedClassicPositions() {
        assertEquals(listOf("P4","C","P3","B","A","P2","Action","P1"),
            pocketPcButtonOrder(true,false).map { labels[it] })
    }
    @Test fun bothLayoutsAndSizesKeepTheirGeometry() {
        for (controller in listOf(false,true)) for (big in listOf(false,true)) {
            val geometry=PocketPcGeometry.calculate(2486,1200,3.5f,big,controller)
            val order=pocketPcButtonOrder(true,controller)
            assertEquals((0..7).toSet(),order.toSet())
            assertEquals(geometry.actions.toSet(),order.map { geometry.actions[it] }.toSet())
            // Switching off restores the same slots; no dimensions or screen bounds change.
            assertEquals((0..7).toList(),pocketPcButtonOrder(false,false))
        }
    }
    @Test fun physicalInputsFollowTheSameSlotPermutation() {
        val keys=listOf(0x11,0xc2,0x0d,0xc1,0xc4,0x10,0xc3,0x20)
        for (controller in listOf(false,true)) {
            keys.forEachIndexed { slot,vk ->
                assertEquals(keys[pocketPcButtonOrder(true,controller)[slot]], pocketPcPhysicalKey(vk,true,controller))
                assertEquals(vk,pocketPcPhysicalKey(vk,false,controller))
            }
            for (vk in listOf(0x25,0x26,0x27,0x28,0x09,0x1b,0x72,0x70))
                assertEquals(vk,pocketPcPhysicalKey(vk,true,controller))
        }
    }
}
