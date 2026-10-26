package com.pockethle.app

internal data class DisplayRect(val left: Int, val top: Int, val width: Int, val height: Int)

/** Pixel-exact rectangle, shared with inverse touch mapping and GL readback. */
internal fun displayRect(viewW: Int, viewH: Int, shownW: Int, shownH: Int, factor: Int): DisplayRect {
    require(shownW > 0 && shownH > 0)
    // Auto uses the largest whole-pixel scale that fits. A viewport smaller
    // than the native screen clips at x1; it never introduces fractional scaling.
    val fit=minOf(viewW/shownW,viewH/shownH).coerceAtLeast(1)
    val scale=if(factor<=0) fit else minOf(fit,factor)
    val w=shownW*scale
    val h=shownH*scale
    return DisplayRect((viewW-w)/2,(viewH-h)/2,w,h)
}
/** VGA uses the same QVGA presentation grid: e.g. 640x480 -> 960x720 at x3.
 * This changes display size only; touch still addresses all native source pixels. */
internal fun pocketPcDisplayRect(viewW: Int,viewH: Int,nativeW: Int,nativeH: Int,factor: Int): DisplayRect {
    val vga=(nativeW==640 && nativeH==480) || (nativeW==480 && nativeH==640)
    val unit=if(vga) 2 else 1
    return displayRect(viewW,viewH,nativeW/unit,nativeH/unit,if(factor<=0) 4 else factor.coerceAtMost(4))
}
internal fun displayPointToGuest(x: Float,y: Float,viewW: Int,viewH: Int,nativeW: Int,nativeH: Int,turn: Int,factor: Int,pocketPc: Boolean=false,fullscreen: Boolean=false): Pair<Int,Int>? {
    if(viewW<=0 || viewH<=0 || nativeW<=0 || nativeH<=0) return null
    val quarter=turn==90 || turn==270
    val w=if(quarter) nativeH else nativeW;val h=if(quarter) nativeW else nativeH
    val rect=if(fullscreen) aspectDisplayRect(viewW,viewH,w,h) else if(pocketPc) pocketPcDisplayRect(viewW,viewH,w,h,factor) else displayRect(viewW,viewH,w,h,factor)
    val dx=x-rect.left;val dy=y-rect.top
    if(dx<0 || dy<0 || dx>=rect.width || dy>=rect.height) return null
    val px=(dx*w/rect.width).toInt().coerceIn(0,w-1);val py=(dy*h/rect.height).toInt().coerceIn(0,h-1)
    return when(turn) { 90 -> py to (w-1-px);180 -> (w-1-px) to (h-1-py);270 -> (h-1-py) to px;else -> px to py }
}

/** Input sources supply display directions; only the central key sender applies this. */
internal fun displayDirectionToGuest(vk:Int,turns:Int):Int {
    val directions=listOf(0x26,0x27,0x28,0x25)
    val index=directions.indexOf(vk)
    return if(index<0) vk else directions[(index-(turns%4)+4)%4]
}
internal fun displayVectorToGuest(x:Float,y:Float,turns:Int):Pair<Float,Float> =
    when(turns%4) { 1->y to -x;2->-x to -y;3->-y to x;else->x to y }

/** Aspect fit shared by controller fullscreen rendering, layout and inverse input. */
internal fun aspectDisplayRect(viewW:Int,viewH:Int,nativeW:Int,nativeH:Int):DisplayRect {
    val w=viewW.coerceAtLeast(1);val h=viewH.coerceAtLeast(1)
    val nw=nativeW.coerceAtLeast(1).toLong();val nh=nativeH.coerceAtLeast(1).toLong()
    val width:Int;val height:Int
    if(w.toLong()*nh<=h.toLong()*nw) {width=w;height=(w*nh/nw).toInt().coerceAtLeast(1)}
    else {height=h;width=(h*nw/nh).toInt().coerceAtLeast(1)}
    return DisplayRect((w-width)/2,(h-height)/2,width,height)
}
