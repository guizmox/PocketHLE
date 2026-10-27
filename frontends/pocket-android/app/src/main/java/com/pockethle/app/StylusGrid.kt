package com.pockethle.app

import android.view.KeyEvent
import org.json.JSONArray
import org.json.JSONObject

// Additional raw keys for grid capture only: do not change normal key fallback.
internal fun gridKeyName(event:KeyEvent):String? = when(event.keyCode) {
    KeyEvent.KEYCODE_SHIFT_LEFT,KeyEvent.KEYCODE_SHIFT_RIGHT -> "Shift"
    KeyEvent.KEYCODE_CTRL_LEFT,KeyEvent.KEYCODE_CTRL_RIGHT -> "Ctrl"
    KeyEvent.KEYCODE_ALT_LEFT,KeyEvent.KEYCODE_ALT_RIGHT -> "Alt"
    KeyEvent.KEYCODE_META_LEFT,KeyEvent.KEYCODE_META_RIGHT -> "Command"
    KeyEvent.KEYCODE_MOVE_HOME -> "Home"
    KeyEvent.KEYCODE_MOVE_END -> "End"
    KeyEvent.KEYCODE_PAGE_UP -> "PageUp"
    KeyEvent.KEYCODE_PAGE_DOWN -> "PageDown"
    KeyEvent.KEYCODE_INSERT -> "Insert"
    else -> InputBindings.keyName(event)
}

internal fun gridControllerName(keyCode:Int):String? = InputBindings.controllerName(keyCode) ?: when(keyCode) {
    KeyEvent.KEYCODE_BUTTON_C -> "AndroidButton:C"
    KeyEvent.KEYCODE_BUTTON_Z -> "AndroidButton:Z"
    in KeyEvent.KEYCODE_BUTTON_1..KeyEvent.KEYCODE_BUTTON_16 -> "AndroidButton:${keyCode-KeyEvent.KEYCODE_BUTTON_1+1}"
    else -> null
}

/** Same serialized schema and display-space coordinates as pocket-library::stylus_grid. */
internal data class GridInput(val source:String,val name:String) {
    fun canonical()=copy(name=if(source=="keyboard") InputBindings.canonical(name).uppercase() else when(name){
        "Back"->"Select";"Guide"->"Mode";"LeftStick"->"LeftThumb";"RightStick"->"RightThumb";else->name
    })
    fun label()=if(source=="keyboard") "Key $name" else name
    fun conflict(fullscreen:String?):String? {
        val input=canonical()
        if(source=="keyboard" && input.name in listOf("ESCAPE","F10","F11","F12")) return "Reserved by PocketHLE (Escape / F10 / F11 / F12)."
        if(source=="controller" && input.name in listOf("Select","Start","Mode","Misc1")) return "Reserved by PocketHLE (fullscreen, filters, recording or system button)."
        if(source=="controller" && fullscreen!=null && input==GridInput("controller",fullscreen).canonical()) return "This input is assigned to fullscreen in PocketHLE settings."
        return null
    }
}
internal data class GridBinding(val cell:Int,val input:GridInput)
internal class StylusGridConfig(var columns:Int=5,var rows:Int=4,val bindings:MutableList<GridBinding> = mutableListOf()) {
    fun json()=JSONObject().apply {
        put("columns",columns);put("rows",rows)
        put("bindings",JSONArray().apply {bindings.forEach { b-> put(JSONObject().apply {
            put("cell",b.cell);put("input",JSONObject().put("source",b.input.source).put("name",b.input.name))
        }) }})
    }
    fun cellFor(input:GridInput,fullscreen:String?):Int? {
        if((columns to rows) !in sizes || input.conflict(fullscreen)!=null) return null
        return bindings.firstOrNull{it.input.canonical()==input.canonical() && it.cell in 0 until columns*rows}?.cell
    }
    fun point(cell:Int)=( ((cell%columns)*2+1)*10000/(columns*2) ) to ( ((cell/columns)*2+1)*10000/(rows*2) )
    fun bind(cell:Int,input:GridInput,fullscreen:String?):String? {
        input.conflict(fullscreen)?.let{return it}
        if((columns to rows) !in sizes || cell !in 0 until columns*rows) return "Invalid grid cell."
        val canonical=input.canonical()
        bindings.removeAll {it.cell==cell || it.input.canonical()==canonical}
        bindings.add(GridBinding(cell,canonical));return null
    }
    companion object {
        val sizes=listOf(4 to 3,5 to 4,6 to 5)
        fun parse(raw:String):StylusGridConfig {
            val obj=runCatching{JSONObject(raw)}.getOrDefault(JSONObject())
            val c=obj.optInt("columns",5);val r=obj.optInt("rows",4)
            val grid=if((c to r) in sizes) StylusGridConfig(c,r) else StylusGridConfig()
            val list=obj.optJSONArray("bindings") ?: return grid
            for(i in 0 until minOf(list.length(),30)) {
                val b=list.optJSONObject(i) ?: continue;val input=b.optJSONObject("input") ?: continue
                val source=input.optString("source");val name=input.optString("name")
                if(source in listOf("keyboard","controller") && name.isNotEmpty()) grid.bind(b.optInt("cell",-1),GridInput(source,name),null)
            }
            return grid
        }
        fun guestPoint(x:Int,y:Int,turns:Int,w:Int,h:Int):Pair<Int,Int> {
            val u=x.coerceIn(0,10000);val v=y.coerceIn(0,10000)
            val p=when(turns%4){1->v to 10000-u;2->10000-u to 10000-v;3->10000-v to u;else->u to v}
            return (p.first*w.coerceIn(1,32767)/10000).coerceIn(0,w.coerceIn(1,32767)-1) to
                (p.second*h.coerceIn(1,32767)/10000).coerceIn(0,h.coerceIn(1,32767)-1)
        }
    }
}
internal data class GridTouch(val phase:Int,val x:Int,val y:Int) // 0 down, 1 move, 2 up
internal class StylusGridRouter(val config:StylusGridConfig,private val fullscreen:String?) {
    private val held=mutableListOf<Pair<GridInput,Int>>()
    private var point:Pair<Int,Int>?=null
    val active get()=point!=null
    fun assigned(input:GridInput)=config.cellFor(input,fullscreen)!=null
    fun input(raw:GridInput,down:Boolean):GridTouch? {
        val input=raw.canonical();val cell=config.cellFor(input,fullscreen) ?: return null
        val old=held.indexOfFirst{it.first==input}
        if(down){if(old>=0)return null;held.add(input to cell)}else{if(old<0)return null;held.removeAt(old)}
        val next=held.lastOrNull()?.second?.let{config.point(it)};val previous=point;point=next
        return when {
            previous==null && next!=null->GridTouch(0,next.first,next.second)
            previous!=null && next==null->GridTouch(2,previous.first,previous.second)
            next!=null && next!=previous->GridTouch(1,next.first,next.second)
            else->null
        }
    }
    fun cancel():GridTouch? {held.clear();val p=point ?: return null;point=null;return GridTouch(2,p.first,p.second)}
}
