package com.pockethle.app

/** Android action remapped to one independently owned subset of fingers.
 * Numeric constants keep the routing policy testable without an Android runtime. */
internal fun routedTouchAction(action: Int, changedId: Int, ownedIds: List<Int>): Int? {
    if (ownedIds.isEmpty()) return null
    val index=ownedIds.indexOf(changedId)
    return when(action) {
        0,5 -> if(index<0) 2 else if(ownedIds.size==1) 0 else 5 or (index shl 8)
        1,6 -> if(index<0) 2 else if(ownedIds.size==1) 1 else 6 or (index shl 8)
        3 -> 3
        else -> 2
    }
}
