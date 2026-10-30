package com.pockethle.app

import android.app.Dialog
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.os.Bundle
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.widget.*
import org.json.JSONObject

/** Local editor: no input is sent to a game and changes commit only with Done. */
internal class StylusControlsDialog(
    context:Context, private val settings:GameSettings, private val config:LauncherConfig,
    private val save:(StylusGridConfig)->Unit,
):Dialog(context) {
    private val testing=settings.controlsMode()=="zones"
    private val grid=StylusGridConfig.parse(settings.pocketpcStylusGridJson)
    private val bindings=InputBindings(config)
    private val fullscreen=JSONObject(config.originalJson).optString("gamepad_fullscreen").takeIf{it.isNotEmpty() && it!="null"}
    private var virtualDialog:Dialog?=null
    private var waiting:Int?=null
    private var selected:Int?=null
    private lateinit var status:TextView
    private lateinit var cells:GridLayout
    private lateinit var preview:TestSurface
    private val arrows=BooleanArray(4)
    private val arrowSources=mutableMapOf<String,Int>()
    private val lastAxis=IntArray(2)
    private var stickX=0f
    private var stickY=0f
    private var axisNavigationAt=0L
    private val portrait:Boolean by lazy {
        val landscape=settings.screen in listOf("landscape","vga_landscape","hpc")
        val turned=settings.rotation in listOf("cw90","ccw90")
        !(landscape xor turned)
    }
    override fun onCreate(state:Bundle?) {
        super.onCreate(state)
        setTitle(if(testing) "Test touch steering" else "Touch zone mapping")
        val pad=(12*context.resources.displayMetrics.density).toInt()
        val content=LinearLayout(context).apply {orientation=LinearLayout.VERTICAL;setPadding(pad,pad,pad,pad)}
        val scroll=ScrollView(context).apply{addView(content)}
        setContentView(scroll)
        status=TextView(context).apply {text=if(testing) "Try arrows / D-pad and the left stick. Release returns to the centre." else "Select a zone, then press a key, button or stick direction. Reserved PocketHLE inputs are refused."}
        content.addView(status)
        if(!testing) {
            val spinner=Spinner(context).apply {
                adapter=ArrayAdapter(context,android.R.layout.simple_spinner_dropdown_item,StylusGridConfig.sizes.map{"${it.first} × ${it.second}"})
                setSelection(StylusGridConfig.sizes.indexOf(grid.columns to grid.rows).coerceAtLeast(0))
            }
            content.addView(spinner)
            spinner.onItemSelectedListener=object:AdapterView.OnItemSelectedListener {
                override fun onNothingSelected(parent:AdapterView<*>?) {}
                override fun onItemSelected(parent:AdapterView<*>?,view:View?,position:Int,id:Long) {
                    val (c,r)=StylusGridConfig.sizes[position]
                    if(c!=grid.columns || r!=grid.rows){grid.columns=c;grid.rows=r;grid.bindings.clear();waiting=null;selected=null;refreshCells();status.text="Grid resized; assignments cleared."}
                }
            }
            cells=GridLayout(context).apply {columnCount=grid.columns;rowCount=grid.rows}
            content.addView(cells,LinearLayout.LayoutParams(-1,-2))
            content.post{refreshCells()}
            val actions=LinearLayout(context)
            actions.addView(Button(context).apply{text="Clear selected";setOnClickListener{selected?.let{cell->grid.bindings.removeAll{it.cell==cell}};waiting=null;refreshCells()}})
            actions.addView(Button(context).apply{text="Clear all";setOnClickListener{grid.bindings.clear();waiting=null;refreshCells()}})
            content.addView(actions)
            content.addView(Button(context).apply{text="Cancel assignment";setOnClickListener{waiting=null;status.text="Assignment cancelled.";refreshCells()}})
        } else {
            preview=TestSurface(context)
            val maxWidth=(context.resources.displayMetrics.widthPixels-pad*4).coerceAtLeast(160)
            val maxHeight=(context.resources.displayMetrics.heightPixels*.55f).toInt()
            val ratio=if(portrait) .75f else 4f/3f
            val width=minOf(maxWidth,(maxHeight*ratio).toInt())
            content.addView(preview,LinearLayout.LayoutParams(width,(width/ratio).toInt()).apply{gravity=android.view.Gravity.CENTER_HORIZONTAL})
        }
        if(testing) content.addView(Button(context).apply{text="Test with touch controls";setOnClickListener{showVirtualControls()}})
        content.addView(Button(context).apply{text="Done";setOnClickListener{if(!testing) save(grid);dismiss()}})
        if(!testing)content.addView(Button(context).apply{text="Cancel";setOnClickListener{dismiss()}})
        window?.setLayout((context.resources.displayMetrics.widthPixels*.94f).toInt(),android.view.WindowManager.LayoutParams.WRAP_CONTENT)
    }
    private fun refreshCells() {
        if(!::cells.isInitialized)return
        val focused=(currentFocus?.tag as? Int)
        cells.removeAllViews();cells.columnCount=grid.columns;cells.rowCount=grid.rows
        val available=(cells.width.takeIf{it>0} ?: (context.resources.displayMetrics.widthPixels*.8f).toInt())
        val ratio=if(portrait) .75f else 4f/3f
        val width=minOf(available,(context.resources.displayMetrics.heightPixels*.5f*ratio).toInt())
        for(cell in 0 until grid.columns*grid.rows) {
            val label=if(waiting==cell) "Press input…" else grid.bindings.firstOrNull{it.cell==cell}?.input?.label() ?: "${cell+1}"
            val button=Button(context).apply {
                text=label;isAllCaps=false;textSize=10f;minWidth=0;minimumWidth=0;minHeight=0;minimumHeight=0
                setPadding(2,2,2,2);tag=cell
                contentDescription="Zone ${cell+1}: $label"
                if(waiting==cell)setBackgroundColor(Color.rgb(40,110,180))
                setOnClickListener{selected=cell;waiting=cell;status.text="Press an input. Back cancels assignment.";refreshCells();showVirtualControls()}
            }
            cells.addView(button,GridLayout.LayoutParams().apply{this.width=width/grid.columns;this.height=(width/ratio/grid.rows).toInt();setMargins(1,1,1,1)})
            if(focused==cell)button.requestFocus()
        }
    }
    /** Reuse the in-game bank and screen geometry; no native session exists here. */
    private fun showVirtualControls() {
        virtualDialog?.dismiss()
        val dialog=object:Dialog(context) {
            override fun dispatchKeyEvent(event:KeyEvent):Boolean {
                if(event.keyCode==KeyEvent.KEYCODE_BACK || event.keyCode==KeyEvent.KEYCODE_ESCAPE) {
                    if(event.action==KeyEvent.ACTION_UP)dismiss()
                    return true
                }
                return this@StylusControlsDialog.dispatchKeyEvent(event)
            }
            override fun onGenericMotionEvent(event:MotionEvent)=this@StylusControlsDialog.onGenericMotionEvent(event)
        }
        virtualDialog=dialog
        val root=PocketPcLayout(context)
        val screen=FrameLayout(context).apply{id=R.id.content_column}
        val bank=PocketPcControls(context).apply{id=R.id.game_controls;bigDirectionalControls=config.pocketpcControlsSize=="big"}
        root.addView(screen);root.addView(bank)
        root.setDisplaySize(if(portrait)240 else 320,if(portrait)320 else 240,4)
        val test=if(testing)TestSurface(context) else null
        if(test!=null)screen.addView(test,FrameLayout.LayoutParams(-1,-1))
        val hint=TextView(context).apply {
            text=if(testing)"Touch steering — release to centre" else "Zone ${(waiting ?: 0)+1}: press a virtual control"
            setTextColor(Color.WHITE);setBackgroundColor(0xaa101824.toInt());gravity=android.view.Gravity.CENTER
        }
        screen.addView(hint,FrameLayout.LayoutParams(-1,-2,android.view.Gravity.TOP))
        screen.addView(Button(context).apply{text="Back";setOnClickListener{dialog.dismiss()}},FrameLayout.LayoutParams(-2,-2,android.view.Gravity.BOTTOM or android.view.Gravity.CENTER_HORIZONTAL))
        fun redraw(){if(test!=null){updatePreview();test.xFraction=preview.xFraction;test.yFraction=preview.yFraction;test.invalidate()}}
        bank.dpad.onDirectionsChanged={_,mask->
            listOf(DpadDirections.UP to 0x26,DpadDirections.RIGHT to 0x27,DpadDirections.DOWN to 0x28,DpadDirections.LEFT to 0x25).forEachIndexed{i,(bit,vk)->
                val down=mask and bit!=0
                if(testing)setArrow("virtual:$vk",i,down) else if(down)virtualGridInput(vk)?.let{capture(it)}
            };redraw()
        }
        bank.aim.onAim={x,y->
            if(testing){stickX=x;stickY=y;redraw()}else {
                val direction=listOf("Left" to -x,"Right" to x,"Up" to -y,"Down" to y).maxByOrNull{it.second}
                if(direction!=null && direction.second>.18f)capture(GridInput("controller","Touch:Stick${direction.first}"))
            }
        }
        val keys=mapOf(R.id.btn_a to 0x11,R.id.btn_b to 0x20,R.id.btn_c to 0x10,R.id.btn_action to 0x0d,
            R.id.btn_ppc_app1 to 0xc1,R.id.btn_ppc_app2 to 0xc2,R.id.btn_ppc_app3 to 0xc3,R.id.btn_ppc_app4 to 0xc4,R.id.btn_soft1 to 0x09)
        keys.forEach{(id,vk)->bank.findViewById<View>(id).setOnTouchListener{v,e->
            v.isPressed=e.actionMasked==MotionEvent.ACTION_DOWN
            if(e.actionMasked==MotionEvent.ACTION_DOWN && !testing)virtualGridInput(vk)?.let{capture(it)}
            true
        }}
        // Host commands stay reserved and cannot be assigned to game zones.
        listOf(R.id.btn_soft2,R.id.btn_turbo,R.id.btn_stop_emulation,R.id.btn_fullscreen,R.id.btn_rotate_game).forEach{bank.findViewById<View>(it).isEnabled=false}
        dialog.setContentView(root)
        dialog.setOnDismissListener {
            bank.clearPressedControls();arrowSources.clear();arrows.fill(false);stickX=0f;stickY=0f
            if(waiting!=null){waiting=null;status.text="Assignment cancelled.";refreshCells()}
            if(testing)updatePreview()
            virtualDialog=null
        }
        dialog.show()
        dialog.window?.setLayout(android.view.WindowManager.LayoutParams.MATCH_PARENT,android.view.WindowManager.LayoutParams.MATCH_PARENT)
    }
    override fun dismiss(){virtualDialog?.dismiss();super.dismiss()}

    override fun dispatchKeyEvent(event:KeyEvent):Boolean {
        val down=event.action==KeyEvent.ACTION_DOWN
        if(event.keyCode==KeyEvent.KEYCODE_BACK || event.keyCode==KeyEvent.KEYCODE_ESCAPE) {
            if(down && event.repeatCount==0){if(waiting!=null){waiting=null;status.text="Assignment cancelled.";refreshCells()}else dismiss()};return true
        }
        val sources=event.device?.sources ?: event.source
        val controller=sources and InputDevice.SOURCE_GAMEPAD==InputDevice.SOURCE_GAMEPAD || sources and InputDevice.SOURCE_JOYSTICK==InputDevice.SOURCE_JOYSTICK
        val name=if(controller)gridControllerName(event.keyCode) else gridKeyName(event)
        if(waiting!=null){
            if(down && event.repeatCount==0 && name!=null)capture(GridInput(if(controller)"controller" else "keyboard",name))
            return true
        }
        if(testing){
            if(controller && name=="East"){if(down)dismiss();return true}
            val vk=if(controller)name?.let{bindings.controllerKey(it)} else bindings.keyboardKey(event)
            val index=when(vk){0x26->0;0x27->1;0x28->2;0x25->3;else->null}
            if(index!=null){setArrow("${event.deviceId}:$name",index,down);updatePreview()}
            return true
        }
        if(controller && name=="East"){if(down)dismiss();return true}
        return super.dispatchKeyEvent(event)
    }
    private fun capture(input:GridInput) {
        val cell=waiting ?: return
        val error=grid.bind(cell,input,fullscreen)
        if(error==null){waiting=null;status.text="Assigned ${input.label()}. Press Done to save.";refreshCells();virtualDialog?.dismiss()}else{status.text=error}
    }
    override fun onGenericMotionEvent(event:MotionEvent):Boolean {
        if(event.action!=MotionEvent.ACTION_MOVE)return super.onGenericMotionEvent(event)
        val controls=listOf("LeftStickLeft" to -event.getAxisValue(MotionEvent.AXIS_X),"LeftStickRight" to event.getAxisValue(MotionEvent.AXIS_X),
            "LeftStickUp" to -event.getAxisValue(MotionEvent.AXIS_Y),"LeftStickDown" to event.getAxisValue(MotionEvent.AXIS_Y),
            "RightStickLeft" to -event.getAxisValue(MotionEvent.AXIS_Z),"RightStickRight" to event.getAxisValue(MotionEvent.AXIS_Z),
            "RightStickUp" to -event.getAxisValue(MotionEvent.AXIS_RZ),"RightStickDown" to event.getAxisValue(MotionEvent.AXIS_RZ),
            "DPadLeft" to -event.getAxisValue(MotionEvent.AXIS_HAT_X),"DPadRight" to event.getAxisValue(MotionEvent.AXIS_HAT_X),
            "DPadUp" to -event.getAxisValue(MotionEvent.AXIS_HAT_Y),"DPadDown" to event.getAxisValue(MotionEvent.AXIS_HAT_Y),
            "LeftTrigger2" to event.getAxisValue(MotionEvent.AXIS_LTRIGGER),"RightTrigger2" to event.getAxisValue(MotionEvent.AXIS_RTRIGGER))
        if(testing){
            stickX=event.getAxisValue(MotionEvent.AXIS_X);stickY=event.getAxisValue(MotionEvent.AXIS_Y)
            listOf("DPadUp","DPadRight","DPadDown","DPadLeft").forEachIndexed{i,n->
                val value=controls.first{it.first==n}.second>.5f
                setArrow("hat:${event.deviceId}:$n",i,value)
            };updatePreview();return true
        }
        val direction=controls.firstOrNull{it.second>.18f}?.first ?: return true
        if(waiting!=null){capture(GridInput("controller",direction));return true}
        if(android.os.SystemClock.uptimeMillis()-axisNavigationAt>250) {
            val focus=when{direction.endsWith("Left")->View.FOCUS_LEFT;direction.endsWith("Right")->View.FOCUS_RIGHT;direction.endsWith("Up")->View.FOCUS_UP;direction.endsWith("Down")->View.FOCUS_DOWN;else->null}
            if(focus!=null){currentFocus?.focusSearch(focus)?.requestFocus();axisNavigationAt=android.os.SystemClock.uptimeMillis()}
        }
        return true
    }
    private fun setArrow(source:String,index:Int,down:Boolean) {
        val was=arrows[index]
        if(down)arrowSources[source]=index else arrowSources.remove(source)
        arrows[index]=arrowSources.values.any{it==index}
        if(was!=arrows[index]) {
            val axis=if(index%2==1)0 else 1
            if(arrows[index])lastAxis[axis]=index else if(arrows[(index+2)%4])lastAxis[axis]=(index+2)%4
        }
    }
    override fun onWindowFocusChanged(hasFocus:Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if(!hasFocus && testing && ::preview.isInitialized){arrowSources.clear();arrows.fill(false);stickX=0f;stickY=0f;updatePreview()}
    }
    private fun updatePreview() {
        fun axis(v:Float):Float {val magnitude=((kotlin.math.abs(v)-.18f)/.82f).coerceIn(0f,1f);return if(v<0)-magnitude else magnitude}
        val x=if(arrows[1] || arrows[3]){if(lastAxis[0]==3)-1f else 1f}else axis(stickX)
        val y=if(arrows[0] || arrows[2]){if(lastAxis[1]==0)-1f else 1f}else axis(stickY)
        preview.xFraction=.5f+x*.25f;preview.yFraction=.5f+y*.25f;preview.invalidate()
    }
    private class TestSurface(context:Context):View(context) {
        var xFraction=.5f;var yFraction=.5f;private val paint=Paint(Paint.ANTI_ALIAS_FLAG)
        override fun onDraw(canvas:Canvas) {
            canvas.drawColor(Color.rgb(18,32,48));paint.color=Color.GRAY;paint.strokeWidth=2f
            canvas.drawLine(width/2f,0f,width/2f,height.toFloat(),paint);canvas.drawLine(0f,height/2f,width.toFloat(),height/2f,paint)
            paint.color=Color.rgb(255,100,65);canvas.drawCircle(xFraction*width,yFraction*height,7*resources.displayMetrics.density,paint)
        }
    }
}
