package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.view.ViewGroup

/** The surface owns only its reserved rectangle, never the controls' touch zones. */
class PocketPcLayout(context: Context,attrs: AttributeSet?=null): ViewGroup(context,attrs) {
    init { setBackgroundColor(0xff080d15.toInt()); isMotionEventSplittingEnabled=true }
    private var nativeWidth=240
    private var nativeHeight=320
    private var scaleLimit=4
    fun setDisplaySize(width: Int,height: Int,limit: Int) {
        if(nativeWidth==width && nativeHeight==height && scaleLimit==limit) return
        nativeWidth=width.coerceAtLeast(1);nativeHeight=height.coerceAtLeast(1);scaleLimit=limit
        requestLayout()
    }
    private fun surfaceRect(room: ControlRect,portrait: Boolean): ControlRect {
        val fit=displayRect(room.w,room.h,nativeWidth,nativeHeight,scaleLimit)
        // Keep x1 clipped inside its room if the device cannot fit native pixels.
        val w=minOf(room.w,fit.width);val h=minOf(room.h,fit.height)
        return ControlRect(room.x+(room.w-w)/2,room.y+if(portrait) 0 else (room.h-h)/2,w,h)
    }
    override fun onMeasure(widthMeasureSpec:Int,heightMeasureSpec:Int) {
        val w=MeasureSpec.getSize(widthMeasureSpec); val h=MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w,h)
        val controls=requireNotNull(findViewById<PocketPcControls>(R.id.game_controls))
        val roomW=(w-paddingLeft-paddingRight).coerceAtLeast(1);val roomH=(h-paddingTop-paddingBottom).coerceAtLeast(1)
        controls.measure(MeasureSpec.makeMeasureSpec(roomW,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(roomH,MeasureSpec.EXACTLY))
        val room=if(controls.visibility==View.GONE) ControlRect(0,0,roomW,roomH) else controls.geometry.screen
        val rect=surfaceRect(room,roomW<=roomH)
        requireNotNull(findViewById<View>(R.id.content_column)).measure(MeasureSpec.makeMeasureSpec(rect.w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(rect.h,MeasureSpec.EXACTLY))
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        val controls=requireNotNull(findViewById<PocketPcControls>(R.id.game_controls))
        val room=if(controls.visibility==View.GONE) ControlRect(0,0,width-paddingLeft-paddingRight,height-paddingTop-paddingBottom) else controls.geometry.screen
        val rect=surfaceRect(room,width<=height)
        requireNotNull(findViewById<View>(R.id.content_column)).layout(paddingLeft+rect.x,paddingTop+rect.y,paddingLeft+rect.x+rect.w,paddingTop+rect.y+rect.h)
        controls.layout(paddingLeft,paddingTop,width-paddingRight,height-paddingBottom)
    }
}
