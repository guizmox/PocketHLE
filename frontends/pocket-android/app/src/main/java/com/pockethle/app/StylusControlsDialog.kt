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

/** Offline touch steering test and read-only grid preview. */
internal class StylusControlsDialog(
    context:Context, private val settings:GameSettings, private val config:LauncherConfig,
    private val viewGrid: Boolean = false,
):Dialog(context) {
    private val testing = !viewGrid
    private val grid=StylusGridConfig.parse(settings.pocketpcStylusGridJson)
    private val bindings=InputBindings(config,false)
    private var virtualDialog: Dialog? = null
    private lateinit var cells:GridLayout
    private lateinit var preview:TestSurface
    private val arrows=BooleanArray(4)
    private val arrowSources=mutableMapOf<String,Int>()
    private val lastAxis=IntArray(2)
    private var stickX=0f
    private var stickY=0f
    private val portrait:Boolean by lazy {
        val landscape = settings.screen in listOf(
            "landscape", "wqvga_landscape", "vga_landscape", "wvga_landscape", "hpc",
        )
        val turned=settings.rotation in listOf("cw90","ccw90")
        !(landscape xor turned)
    }
    override fun onCreate(state:Bundle?) {
        super.onCreate(state)
        setTitle(if(testing) "Test touch steering" else "View Grid")
        val pad=(12*context.resources.displayMetrics.density).toInt()
        val content=LinearLayout(context).apply {orientation=LinearLayout.VERTICAL;setPadding(pad,pad,pad,pad)}
        val scroll=ScrollView(context).apply{addView(content)}
        setContentView(scroll)
        if (viewGrid) {
            content.addView(TextView(context).apply { text = "${grid.columns} × ${grid.rows}" })
            cells = GridLayout(context).apply {
                columnCount = grid.columns
                rowCount = grid.rows
            }
            content.addView(cells, LinearLayout.LayoutParams(-1, -2))
            content.post { showGridCells() }
        } else {
            content.addView(TextView(context).apply {
                text = "Try arrows / D-pad and the left stick. Release returns to the centre."
            })
            preview=TestSurface(context)
            val maxWidth=(context.resources.displayMetrics.widthPixels-pad*4).coerceAtLeast(160)
            val maxHeight=(context.resources.displayMetrics.heightPixels*.55f).toInt()
            val ratio=if(portrait) .75f else 4f/3f
            val width=minOf(maxWidth,(maxHeight*ratio).toInt())
            content.addView(preview,LinearLayout.LayoutParams(width,(width/ratio).toInt()).apply{gravity=android.view.Gravity.CENTER_HORIZONTAL})
        }
        if(testing) content.addView(Button(context).apply{text="Test with touch controls";setOnClickListener{showVirtualControls()}})
        content.addView(Button(context).apply {
            text = "Done"
            setOnClickListener { dismiss() }
            if (viewGrid) requestFocus()
        })
        window?.setLayout((context.resources.displayMetrics.widthPixels*.94f).toInt(),android.view.WindowManager.LayoutParams.WRAP_CONTENT)
    }
    private fun showGridCells() {
        val available = cells.width.takeIf { it > 0 }
            ?: (context.resources.displayMetrics.widthPixels * .8f).toInt()
        val maxHeight = (context.resources.displayMetrics.heightPixels * .5f).toInt()
        val side = (minOf(available / grid.columns, maxHeight / grid.rows) - 2).coerceAtLeast(1)
        for (cell in 0 until grid.columns * grid.rows) {
            val label = grid.bindings.firstOrNull { it.cell == cell }?.input
                ?.let(ControllerLabels::grid) ?: (cell + 1).toString()
            val tile = TextView(context).apply {
                text = label
                gravity = android.view.Gravity.CENTER
                textSize = 10f
                setBackgroundColor(0x337f7f7f)
                contentDescription = "Zone ${cell + 1}: $label"
            }
            cells.addView(tile, GridLayout.LayoutParams().apply {
                width = side
                height = side
                setMargins(1, 1, 1, 1)
            })
        }
    }

    /** Reuse the in-game bank and screen geometry; no native session exists here. */
    private fun showVirtualControls() {
        virtualDialog?.dismiss()
        val dialog=object:Dialog(context) {
            override fun dispatchKeyEvent(event:KeyEvent):Boolean {
                ControllerLabels.observe(event)
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
        val bank=PocketPcControls(context).apply{id=R.id.game_controls;configure(config, settings)}
        root.addView(screen);root.addView(bank)
        root.setDisplaySize(if(portrait)240 else 320,if(portrait)320 else 240)
        val test = TestSurface(context)
        screen.addView(test,FrameLayout.LayoutParams(-1,-1))
        val hint=TextView(context).apply {
            text = "Touch steering — release to centre"
            setTextColor(Color.WHITE);setBackgroundColor(0xaa101824.toInt());gravity=android.view.Gravity.CENTER
        }
        screen.addView(hint,FrameLayout.LayoutParams(-1,-2,android.view.Gravity.TOP))
        screen.addView(Button(context).apply{text="Back";setOnClickListener{dialog.dismiss()}},FrameLayout.LayoutParams(-2,-2,android.view.Gravity.BOTTOM or android.view.Gravity.CENTER_HORIZONTAL))
        fun redraw(){updatePreview();test.xFraction=preview.xFraction;test.yFraction=preview.yFraction;test.invalidate()}
        bank.dpad.onDirectionsChanged={_,mask->
            listOf(DpadDirections.UP to 0x26,DpadDirections.RIGHT to 0x27,DpadDirections.DOWN to 0x28,DpadDirections.LEFT to 0x25).forEachIndexed{i,(bit,vk)->
                val down=mask and bit!=0
                setArrow("virtual:$vk",i,down)
            };redraw()
        }
        bank.aim.onAim = { x, y ->
            stickX = x
            stickY = y
            redraw()
        }
        // Host commands stay reserved and cannot be assigned to game zones.
        listOf(R.id.btn_soft2,R.id.btn_turbo,R.id.btn_stop_emulation,R.id.btn_fullscreen,R.id.btn_rotate_game).forEach{bank.findViewById<View>(it).isEnabled=false}
        dialog.setContentView(root)
        dialog.setOnDismissListener {
            bank.clearPressedControls();arrowSources.clear();arrows.fill(false);stickX=0f;stickY=0f
            updatePreview()
            virtualDialog=null
        }
        dialog.show()
        dialog.window?.setLayout(android.view.WindowManager.LayoutParams.MATCH_PARENT,android.view.WindowManager.LayoutParams.MATCH_PARENT)
    }
    override fun dismiss(){virtualDialog?.dismiss();super.dismiss()}

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        ControllerLabels.observe(event)
        val down = event.action == KeyEvent.ACTION_DOWN
        if (event.keyCode in listOf(KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_BUTTON_B)) {
            if (down && event.repeatCount == 0) dismiss()
            return true
        }
        if (viewGrid) return super.dispatchKeyEvent(event)
        val sources = event.device?.sources ?: event.source
        val controller = sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
            sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        val name = if (controller) gridControllerName(event.keyCode) else gridKeyName(event)
        val vk = if (controller) name?.let { bindings.controllerKey(it) } else bindings.keyboardKey(event)
        val index = when (vk) {
            0x26 -> 0
            0x27 -> 1
            0x28 -> 2
            0x25 -> 3
            else -> return super.dispatchKeyEvent(event)
        }
        setArrow("${event.deviceId}:$name", index, down)
        updatePreview()
        return true
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (viewGrid || event.action != MotionEvent.ACTION_MOVE) return super.onGenericMotionEvent(event)
        stickX = event.getAxisValue(MotionEvent.AXIS_X)
        stickY = event.getAxisValue(MotionEvent.AXIS_Y)
        val hatX = event.getAxisValue(MotionEvent.AXIS_HAT_X)
        val hatY = event.getAxisValue(MotionEvent.AXIS_HAT_Y)
        listOf(-hatY, hatX, hatY, -hatX).forEachIndexed { index, value ->
            setArrow("hat:${event.deviceId}:$index", index, value > .5f)
        }
        updatePreview()
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
