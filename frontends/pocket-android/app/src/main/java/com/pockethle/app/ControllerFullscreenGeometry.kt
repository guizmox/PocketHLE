package com.pockethle.app

internal data class ControllerFullscreenGeometry(val screen: ControlRect, val exit: ControlRect)

/** Fit the displayed orientation, leaving the exit target entirely outside the game. */
internal fun controllerFullscreenGeometry(width: Int, height: Int, nativeW: Int, nativeH: Int, density: Float): ControllerFullscreenGeometry {
    val w=width.coerceAtLeast(1); val h=height.coerceAtLeast(1)
    val button=(40*density).toInt().coerceAtLeast(1).coerceAtMost(minOf(w,h))
    fun fit(room: ControlRect): ControlRect {
        val r=aspectDisplayRect(room.w,room.h,nativeW,nativeH)
        return ControlRect(room.x+r.left,room.y+r.top,r.width,r.height)
    }
    var screen=fit(ControlRect(0,0,w,h))
    fun margins(s: ControlRect)=listOf(
        ControlRect(0,0,s.x,h), ControlRect(s.x+s.w,0,w-s.x-s.w,h),
        ControlRect(0,0,w,s.y), ControlRect(0,s.y+s.h,w,h-s.y-s.h))
    var margin=margins(screen).filter { it.w>=button && it.h>=button }.maxByOrNull { it.w.toLong()*it.h }
    if(margin==null) {
        // Matching aspect ratios have no natural letterbox. Reserve one slim
        // external strip rather than covering game pixels or losing the exit.
        screen=if(w>h) fit(ControlRect(button,0,(w-button).coerceAtLeast(1),h))
            else fit(ControlRect(0,button,w,(h-button).coerceAtLeast(1)))
        margin=if(w>h) ControlRect(0,0,button,h) else ControlRect(0,0,w,button)
    }
    return ControllerFullscreenGeometry(screen,ControlRect(margin.x+(margin.w-button)/2,margin.y+(margin.h-button)/2,button,button))
}
