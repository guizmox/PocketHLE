package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.view.ViewGroup
import android.view.MotionEvent

/** The surface owns only its reserved rectangle, never the controls' touch zones. */
class PocketPcLayout(context: Context,attrs: AttributeSet?=null): ViewGroup(context,attrs) {
    init { setBackgroundColor(0xff080d15.toInt()); isMotionEventSplittingEnabled=true }
    private val surfaceFingers=mutableSetOf<Int>()
    private val controlFingers=mutableSetOf<Int>()

    // A full-window controls overlay cannot rely on ordinary sibling hit tests:
    // Android adds a second finger to its existing touch target even if that
    // finger lands in the overlay's empty game area. Own fingers at this root
    // and send independent streams to the surface and the control bank.
    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        val content=findViewById<View>(R.id.content_column) ?: return super.dispatchTouchEvent(event)
        val controls=findViewById<View>(R.id.game_controls) ?: return super.dispatchTouchEvent(event)
        val changed=event.getPointerId(event.actionIndex)
        if(event.actionMasked==MotionEvent.ACTION_DOWN) {
            surfaceFingers.clear();controlFingers.clear()
        }
        if(event.actionMasked==MotionEvent.ACTION_DOWN || event.actionMasked==MotionEvent.ACTION_POINTER_DOWN) {
            val x=event.getX(event.actionIndex);val y=event.getY(event.actionIndex)
            if(x>=content.left && x<content.right && y>=content.top && y<content.bottom) surfaceFingers.add(changed)
            else if(controls.visibility==View.VISIBLE) controlFingers.add(changed)
        }
        fun dispatch(target: View, owned: Set<Int>) {
            val indices=(0 until event.pointerCount).filter { event.getPointerId(it) in owned }
            val ids=indices.map { event.getPointerId(it) }
            val action=routedTouchAction(event.actionMasked,changed,ids) ?: return
            val properties=Array(indices.size) { MotionEvent.PointerProperties().also { p -> event.getPointerProperties(indices[it],p) } }
            val coords=Array(indices.size) { MotionEvent.PointerCoords().also { c -> event.getPointerCoords(indices[it],c) } }
            val split=MotionEvent.obtain(event.downTime,event.eventTime,action,indices.size,properties,coords,
                event.metaState,event.buttonState,event.xPrecision,event.yPrecision,event.deviceId,event.edgeFlags,event.source,event.flags)
            split.offsetLocation(-target.left.toFloat(),-target.top.toFloat())
            try { target.dispatchTouchEvent(split) } finally { split.recycle() }
        }
        dispatch(content,surfaceFingers)
        if(controls.visibility==View.VISIBLE) dispatch(controls,controlFingers)
        if(event.actionMasked==MotionEvent.ACTION_UP || event.actionMasked==MotionEvent.ACTION_POINTER_UP) {
            surfaceFingers.remove(changed);controlFingers.remove(changed)
        }
        if(event.actionMasked==MotionEvent.ACTION_UP || event.actionMasked==MotionEvent.ACTION_CANCEL) {
            surfaceFingers.clear();controlFingers.clear()
        }
        return true
    }
    private var nativeWidth=240
    private var nativeHeight=320
    private var scaleLimit=4
    fun setDisplaySize(width: Int,height: Int,limit: Int) {
        if(nativeWidth==width && nativeHeight==height && scaleLimit==limit) return
        nativeWidth=width.coerceAtLeast(1);nativeHeight=height.coerceAtLeast(1);scaleLimit=limit
        requestLayout()
    }
    private fun surfaceRect(room: ControlRect): ControlRect {
        val fit=pocketPcDisplayRect(room.w,room.h,nativeWidth,nativeHeight,scaleLimit)
        // Keep x1 clipped inside its room if the device cannot fit native pixels.
        val w=minOf(room.w,fit.width);val h=minOf(room.h,fit.height)
        return ControlRect(room.x+(room.w-w)/2,room.y+(room.h-h)/2,w,h)
    }
    override fun onMeasure(widthMeasureSpec:Int,heightMeasureSpec:Int) {
        val w=MeasureSpec.getSize(widthMeasureSpec); val h=MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w,h)
        val controls=requireNotNull(findViewById<PocketPcControls>(R.id.game_controls))
        val roomW=(w-paddingLeft-paddingRight).coerceAtLeast(1);val roomH=(h-paddingTop-paddingBottom).coerceAtLeast(1)
        controls.measure(MeasureSpec.makeMeasureSpec(roomW,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(roomH,MeasureSpec.EXACTLY))
        val room=if(controls.visibility==View.GONE) ControlRect(0,0,roomW,roomH) else controls.geometry.screen
        val rect=surfaceRect(room)
        requireNotNull(findViewById<View>(R.id.content_column)).measure(MeasureSpec.makeMeasureSpec(rect.w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(rect.h,MeasureSpec.EXACTLY))
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        val controls=requireNotNull(findViewById<PocketPcControls>(R.id.game_controls))
        val room=if(controls.visibility==View.GONE) ControlRect(0,0,width-paddingLeft-paddingRight,height-paddingTop-paddingBottom) else controls.geometry.screen
        val rect=surfaceRect(room)
        requireNotNull(findViewById<View>(R.id.content_column)).layout(paddingLeft+rect.x,paddingTop+rect.y,paddingLeft+rect.x+rect.w,paddingTop+rect.y+rect.h)
        controls.layout(paddingLeft,paddingTop,width-paddingRight,height-paddingBottom)
    }
}
