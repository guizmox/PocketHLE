package com.pockethle.app

/** IDs retain their meaning; only their existing layout slots change. */
internal fun pocketPcButtonOrder(swapped: Boolean, controller: Boolean): List<Int> = when {
    !swapped -> (0..7).toList()
    // Controller: B/C | Action/A; diamond P3/P2/P4/P1.
    controller -> listOf(6, 5, 1, 7, 0, 3, 2, 4)
    // Classic: exchange the left and right four-button banks.
    else -> listOf(4, 5, 6, 7, 0, 1, 2, 3)
}

/** Apply exactly the same slot permutation to configured keyboard/gamepad bindings. */
internal fun pocketPcPhysicalKey(vk: Int, swapped: Boolean, controller: Boolean): Int {
    if (!swapped) return vk
    val keys=listOf(0x11,0xc2,0x0d,0xc1,0xc4,0x10,0xc3,0x20)
    val slot=keys.indexOf(vk)
    return if (slot < 0) vk else keys[pocketPcButtonOrder(true,controller)[slot]]
}
