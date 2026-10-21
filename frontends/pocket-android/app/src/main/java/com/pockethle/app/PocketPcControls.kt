package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.view.ViewGroup
import android.widget.Button

/** Separate touch targets; blank areas let the game surface receive the stylus. */
class PocketPcControls(context: Context, attrs: AttributeSet? = null) : ViewGroup(context, attrs) {
    private val dp=resources.displayMetrics.density
    private val style=ConsoleControlStyle(dp)
    internal val aim=StylusStickView(context)
    val dpad=TouchDpad(context)
    private val keys=linkedMapOf<Int,Button>()
    private val actions=listOf(R.id.btn_a,R.id.btn_ppc_app2,R.id.btn_action,R.id.btn_ppc_app1,
        R.id.btn_ppc_app4,R.id.btn_c,R.id.btn_ppc_app3,R.id.btn_b)
    private val auxiliary=listOf(R.id.btn_soft1,R.id.btn_soft2,R.id.btn_turbo,R.id.btn_stop_emulation,R.id.btn_rotate_game)
    internal var geometry=PocketPcGeometry.calculate(1,1,dp)
    init {
        layoutDirection=View.LAYOUT_DIRECTION_LTR
        isMotionEventSplittingEnabled=true
        fun button(id: Int,label: String,host: Boolean=false) {
            val b=Button(context).apply {
                this.id=id; text=label; textSize=if(host) 13f else 16f; contentDescription=label
                minWidth=0; minimumWidth=0; minHeight=0; minimumHeight=0; setPadding(0,0,0,0)
                setTextColor(0xffffffff.toInt()); background=style.backgroundShape(host)
                backgroundTintList=null; stateListAnimator=null
            }
            keys[id]=b;addView(b)
        }
        actions.zip(listOf("A","P2","Action","P1","P4","C","P3","B")).forEach { (id,label)->button(id,label) }
        auxiliary.zip(listOf("Tab","Esc","Turbo","Exit","↻")).forEachIndexed { index,(id,label)->button(id,label,index>=3) }
        keys.getValue(R.id.btn_rotate_game).contentDescription="Rotate control layout"
        addView(dpad);addView(aim)
    }
    fun clearPressedControls() { keys.values.forEach { it.isPressed=false }; dpad.clearInput(); aim.clearInput() }
    override fun onMeasure(widthMeasureSpec: Int,heightMeasureSpec: Int) {
        val w=MeasureSpec.getSize(widthMeasureSpec);val h=MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w,h);geometry=PocketPcGeometry.calculate(w,h,dp)
        fun measure(view: View,rect: ControlRect) { view.measure(MeasureSpec.makeMeasureSpec(rect.w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(rect.h,MeasureSpec.EXACTLY)) }
        actions.forEachIndexed { i,id->measure(keys.getValue(id),geometry.actions[i]) }
        auxiliary.forEachIndexed { i,id->measure(keys.getValue(id),geometry.auxiliary[i]) }
        measure(dpad,geometry.dpad);measure(aim,geometry.stick)
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        fun place(view: View,rect: ControlRect) { view.layout(rect.x,rect.y,rect.x+rect.w,rect.y+rect.h) }
        actions.forEachIndexed { i,id->place(keys.getValue(id),geometry.actions[i]) }
        auxiliary.forEachIndexed { i,id->place(keys.getValue(id),geometry.auxiliary[i]) }
        place(dpad,geometry.dpad);place(aim,geometry.stick)
    }
}
