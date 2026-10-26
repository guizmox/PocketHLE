package com.pockethle.app

import android.content.Context
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.appcompat.widget.AppCompatImageButton

/** Reuses the existing display container; guest geometry and controls are untouched. */
internal class ControllerFullscreenHost(context: Context, private val originalRoot: View, onExit: () -> Unit): FrameLayout(context) {
    private var screen: View?=null
    private var parentBefore: ViewGroup?=null
    private var paramsBefore: ViewGroup.LayoutParams?=null
    private var indexBefore=0
    private var nativeW=320
    private var nativeH=240
    val active: Boolean get()=screen!=null
    private val exit=AppCompatImageButton(context).apply {
        setImageResource(R.drawable.ic_fullscreen_exit)
        setColorFilter(android.graphics.Color.WHITE)
        setBackgroundColor(0xb0303640.toInt())
        contentDescription="Exit fullscreen"
        setPadding(8,8,8,8)
        visibility=View.GONE
        setOnClickListener { onExit() }
    }
    init {
        setBackgroundColor(0xff080d15.toInt())
        addView(originalRoot,LayoutParams(LayoutParams.MATCH_PARENT,LayoutParams.MATCH_PARENT))
        addView(exit,LayoutParams(40,40))
    }
    fun setDisplaySize(width:Int,height:Int) {
        if(nativeW==width && nativeH==height) return
        nativeW=width.coerceAtLeast(1);nativeH=height.coerceAtLeast(1)
        if(active) requestLayout()
    }
    fun enter(display:View) {
        if(active) return
        val parent=display.parent as ViewGroup
        parentBefore=parent;paramsBefore=display.layoutParams;indexBefore=parent.indexOfChild(display)
        parent.removeView(display)
        originalRoot.visibility=View.GONE
        screen=display
        addView(display,0,LayoutParams(LayoutParams.MATCH_PARENT,LayoutParams.MATCH_PARENT))
        exit.visibility=View.VISIBLE;exit.bringToFront();requestLayout()
    }
    fun leave() {
        val display=screen ?: return
        removeView(display)
        screen=null
        requireNotNull(parentBefore).addView(display,indexBefore,requireNotNull(paramsBefore))
        parentBefore=null;paramsBefore=null
        originalRoot.visibility=View.VISIBLE;exit.visibility=View.GONE;requestLayout()
    }
    private fun geometry()=controllerFullscreenGeometry(measuredWidth,measuredHeight,nativeW,nativeH,resources.displayMetrics.density)
    override fun onMeasure(widthMeasureSpec:Int,heightMeasureSpec:Int) {
        super.onMeasure(widthMeasureSpec,heightMeasureSpec)
        if(!active) return
        val g=geometry()
        fun measure(view:View,r:ControlRect) { view.measure(MeasureSpec.makeMeasureSpec(r.w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(r.h,MeasureSpec.EXACTLY)) }
        measure(requireNotNull(screen),g.screen);measure(exit,g.exit)
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        super.onLayout(changed,l,t,r,b)
        if(!active) return
        val g=geometry()
        fun place(view:View,rect:ControlRect) {view.layout(rect.x,rect.y,rect.x+rect.w,rect.y+rect.h)}
        place(requireNotNull(screen),g.screen);place(exit,g.exit)
    }
}
