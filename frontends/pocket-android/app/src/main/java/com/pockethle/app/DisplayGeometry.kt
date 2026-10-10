package com.pockethle.app

internal data class DisplayRect(val left: Int, val top: Int, val width: Int, val height: Int)

/** Pixel-exact rectangle, shared with inverse touch mapping and GL readback. */
internal fun displayRect(viewW: Int, viewH: Int, shownW: Int, shownH: Int, factor: Int): DisplayRect {
    val fit=minOf(viewW.toFloat()/shownW,viewH.toFloat()/shownH)
    val scale=if(factor==0) fit else minOf(fit,factor.toFloat())
    val w=(shownW*scale).toInt().coerceIn(1,viewW.coerceAtLeast(1))
    val h=(shownH*scale).toInt().coerceIn(1,viewH.coerceAtLeast(1))
    return DisplayRect((viewW-w)/2,(viewH-h)/2,w,h)
}
internal fun displayPointToGuest(x: Float,y: Float,viewW: Int,viewH: Int,nativeW: Int,nativeH: Int,turn: Int,factor: Int): Pair<Int,Int>? {
    if(viewW<=0 || viewH<=0 || nativeW<=0 || nativeH<=0) return null
    val quarter=turn==90 || turn==270
    val w=if(quarter) nativeH else nativeW;val h=if(quarter) nativeW else nativeH
    val rect=displayRect(viewW,viewH,w,h,factor)
    val dx=x-rect.left;val dy=y-rect.top
    if(dx<0 || dy<0 || dx>=rect.width || dy>=rect.height) return null
    val px=(dx*w/rect.width).toInt().coerceIn(0,w-1);val py=(dy*h/rect.height).toInt().coerceIn(0,h-1)
    return when(turn) { 90 -> py to (w-1-px);180 -> (w-1-px) to (h-1-py);270 -> (h-1-py) to px;else -> px to py }
}
