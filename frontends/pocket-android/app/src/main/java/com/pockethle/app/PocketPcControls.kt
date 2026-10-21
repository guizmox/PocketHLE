package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.ViewGroup
import android.widget.Button

/** Shared portrait/landscape grouping: movement left, aiming right, actions centre. */
class PocketPcControls(context: Context, attrs: AttributeSet? = null) : ViewGroup(context, attrs) {
    private val dp=resources.displayMetrics.density
    internal val aim=StylusStickView(context)
    private val keys=linkedMapOf<Int,Button>()
    private val directions=listOf(R.id.btn_up,R.id.btn_down,R.id.btn_left,R.id.btn_right)
    private val actions=listOf(R.id.btn_action,R.id.btn_a,R.id.btn_b,R.id.btn_c,R.id.btn_soft1,R.id.btn_soft2,
        R.id.btn_ppc_app1,R.id.btn_ppc_app2,R.id.btn_ppc_app3,R.id.btn_ppc_app4)
    init {
        fun button(id: Int, label: String, description: String=label) {
            val b=Button(context).apply { this.id=id; text=label; textSize=12f; contentDescription=description
                minWidth=0; minimumWidth=0; minHeight=0; minimumHeight=0; setPadding(0,0,0,0)
                setTextColor(0xffffffff.toInt()); setBackgroundResource(R.drawable.bg_gamepad_button) }
            keys[id]=b; addView(b)
        }
        button(R.id.btn_up,"▲","Move up");button(R.id.btn_down,"▼","Move down")
        button(R.id.btn_left,"◀","Move left");button(R.id.btn_right,"▶","Move right")
        button(R.id.btn_turbo,"Turbo")
        listOf("Action","A","B","C","Tab","Esc","P1","P2","P3","P4").forEachIndexed { i,s -> button(actions[i],s) }
        addView(aim)
    }
    override fun onMeasure(widthMeasureSpec: Int,heightMeasureSpec: Int) {
        val w=MeasureSpec.getSize(widthMeasureSpec); val narrow=w/dp<440f
        val h=((if(narrow) 236 else 164)*dp).toInt()
        setMeasuredDimension(w,resolveSize(h,heightMeasureSpec))
        val side=(minOf(112f,w/dp*.30f)*dp).toInt()
        val centre=(w-2*side).coerceAtLeast(1);val cols=if(narrow) 2 else 4
        keys.forEach { (id,b) ->
            val bw=if(id in actions) (centre/cols-4*dp).toInt().coerceAtLeast(1) else if(id==R.id.btn_turbo) (side*.6f).toInt() else (side/3f-2*dp).toInt().coerceAtLeast(1)
            b.measure(MeasureSpec.makeMeasureSpec(bw,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec((36*dp).toInt(),MeasureSpec.EXACTLY))
        }
        aim.measure(MeasureSpec.makeMeasureSpec(side,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(side,MeasureSpec.EXACTLY))
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        val side=aim.measuredWidth;val centre=width-2*side;val cols=if(width/dp<440f) 2 else 4
        fun place(id:Int,x:Int,y:Int) { keys[id]?.let { it.layout(x,y,x+it.measuredWidth,y+it.measuredHeight) } }
        val cell=side/3;val top=(40*dp).toInt()
        place(R.id.btn_turbo,side-(side*.6f).toInt(),0)
        place(directions[0],cell,top);place(directions[1],cell,top+2*cell)
        place(directions[2],0,top+cell);place(directions[3],2*cell,top+cell)
        actions.forEachIndexed { i,id -> place(id,side+(i%cols)*centre/cols+(2*dp).toInt(),(i/cols)*(40*dp).toInt()+(4*dp).toInt()) }
        aim.layout(width-side,top,width,top+side)
    }
}
