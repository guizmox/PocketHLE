package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.view.MotionEvent
import android.view.ViewGroup
import android.widget.Button

/** Separate touch targets; blank areas let the game surface receive the stylus. */
class PocketPcControls(context: Context, attrs: AttributeSet? = null) : ViewGroup(context, attrs) {
    private val dp=resources.displayMetrics.density
    private val style=ConsoleControlStyle(dp)
    internal val aim=StylusStickView(context)
    private val aimTouchTarget = object : android.widget.FrameLayout(context) {
        init { isMotionEventSplittingEnabled = true; addView(aim, LayoutParams(-1, -1)) }
        override fun dispatchTouchEvent(event: MotionEvent): Boolean {
            if (controllerLayout && event.actionMasked == MotionEvent.ACTION_DOWN
                && !ovalTouchContains(event.x, event.y, width, height)) return false
            return super.dispatchTouchEvent(event)
        }
    }
    val dpad=TouchDpad(context)
    private val keys=linkedMapOf<Int,Button>()
    private val originalActions=listOf(R.id.btn_a,R.id.btn_ppc_app2,R.id.btn_action,R.id.btn_ppc_app1,
        R.id.btn_ppc_app4,R.id.btn_c,R.id.btn_ppc_app3,R.id.btn_b)
    private val actions: List<Int>
        get() = pocketPcButtonOrder(swapButtonGroups, controllerLayout).map { originalActions[it] }
    private fun isDiamond(id: Int) = actions.indexOf(id) in listOf(0, 2, 5, 7)
    var swapButtonGroups: Boolean = false
        set(value) {
            if (field == value) return
            clearPressedControls(); field=value; updateButtonShapes(); requestLayout()
        }
    private val auxiliary=listOf(R.id.btn_soft1,R.id.btn_soft2,R.id.btn_turbo,R.id.btn_stop_emulation,R.id.btn_fullscreen,R.id.btn_rotate_game)
    internal var geometry=PocketPcGeometry.calculate(1,1,dp)
    init {
        layoutDirection=View.LAYOUT_DIRECTION_LTR
        isMotionEventSplittingEnabled=true
        fun button(id: Int,label: String,host: Boolean=false) {
            val b=object : Button(context) {
                override fun dispatchTouchEvent(event: MotionEvent): Boolean {
                    // Reject empty oval corners before OnTouchListener consumes
                    // DOWN. ViewGroup can then try the adjacent action button.
                    // Once accepted, MOVE/UP/CANCEL keep their existing owner.
                    if (controllerLayout && isDiamond(id)
                        && event.actionMasked == MotionEvent.ACTION_DOWN
                        && !ovalTouchContains(event.x, event.y, width, height)) return false
                    return super.dispatchTouchEvent(event)
                }
            }.apply {
                this.id=id; text=label; textSize=if(host) 13f else 16f; contentDescription=label
                minWidth=0; minimumWidth=0; minHeight=0; minimumHeight=0; setPadding(0,0,0,0)
                setTextColor(0xffffffff.toInt()); background=style.backgroundShape(host)
                backgroundTintList=null; stateListAnimator=null
            }
            keys[id]=b;addView(b)
        }
        originalActions.zip(listOf("A","P2","Action","P1","P4","C","P3","B")).forEach { (id,label)->button(id,label) }
        auxiliary.zip(listOf("Tab","Esc","Turbo","Exit","Full","↻")).forEachIndexed { index,(id,label)->button(id,label,index>=3) }
        keys.getValue(R.id.btn_rotate_game).contentDescription="Rotate game screen"
        keys.getValue(R.id.btn_rotate_game).textSize=24f
        addView(dpad);addView(aimTouchTarget)
    }
    var controllerLayout: Boolean = false
        set(value) {
            if (field == value) return
            clearPressedControls(); field = value
            updateButtonShapes()
            keys.getValue(R.id.btn_action).text = if (value) "Act." else "Action"
            auxiliary.forEach { id -> keys.getValue(id).textSize = if (value) 11f else if (id == R.id.btn_rotate_game) 24f else 13f }
            requestLayout()
        }
    var bigDirectionalControls: Boolean = false
        set(value) {
            if (field == value) return
            clearPressedControls()
            field = value
            requestLayout()
        }
    private fun updateButtonShapes() {
        actions.forEach { id -> keys.getValue(id).apply {
            background=style.backgroundShape(oval=controllerLayout && isDiamond(id)); textSize=16f
        } }
    }
    fun configure(config: LauncherConfig, settings: GameSettings = GameSettings.default()) {
        swapButtonGroups = settings.pocketpcSwapButtonGroups
        controllerLayout = config.pocketpcTouchLayout == "controller"
        bigDirectionalControls = config.pocketpcControlsSize == "big"
    }
    fun clearPressedControls() { keys.values.forEach { it.isPressed=false }; dpad.clearInput(); aim.clearInput() }
    override fun onMeasure(widthMeasureSpec: Int,heightMeasureSpec: Int) {
        val w=MeasureSpec.getSize(widthMeasureSpec);val h=MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension(w,h);geometry=PocketPcGeometry.calculate(w,h,dp,bigDirectionalControls,controllerLayout)
        fun measure(view: View,rect: ControlRect) { view.measure(MeasureSpec.makeMeasureSpec(rect.w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(rect.h,MeasureSpec.EXACTLY)) }
        if (controllerLayout) {
            (actions + auxiliary).forEach { id ->
                val button = keys.getValue(id)
                val rect = if (id in actions) geometry.actions[actions.indexOf(id)] else geometry.auxiliary[auxiliary.indexOf(id)]
                val desired = (if (id in actions) 16f else 11f) * resources.displayMetrics.scaledDensity
                val pixels = minOf(desired, rect.h * .32f, rect.w / (button.text.length.coerceAtLeast(1) * .65f))
                button.setTextSize(android.util.TypedValue.COMPLEX_UNIT_PX, pixels.coerceAtLeast(1f))
            }
        }
        actions.forEachIndexed { i,id->measure(keys.getValue(id),geometry.actions[i]) }
        auxiliary.forEachIndexed { i,id->measure(keys.getValue(id),geometry.auxiliary[i]) }
        measure(dpad,geometry.dpad);measure(aimTouchTarget,geometry.stick)
    }
    override fun onLayout(changed:Boolean,l:Int,t:Int,r:Int,b:Int) {
        fun place(view: View,rect: ControlRect) { view.layout(rect.x,rect.y,rect.x+rect.w,rect.y+rect.h) }
        actions.forEachIndexed { i,id->place(keys.getValue(id),geometry.actions[i]) }
        auxiliary.forEachIndexed { i,id->place(keys.getValue(id),geometry.auxiliary[i]) }
        place(dpad,geometry.dpad);place(aimTouchTarget,geometry.stick)
    }
}
