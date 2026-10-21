package com.pockethle.app

import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.hypot

internal class StylusBindings(config: LauncherConfig) {
    private val json=JSONObject(config.originalJson).optJSONObject("stylus_stick") ?: JSONObject()
    val enabled=json.optBoolean("enabled",true)
    val speed=json.optInt("speed",240).coerceIn(30,1200)
    val keyboard=Array(4) { i -> names("keyboard",i,arrayOf("O","L","K","M")[i]) }
    val controller=Array(4) { i -> names("controller",i,"RightStick"+directions[i]) }
    private fun names(kind:String,i:Int,fallback:String):List<String> {
        val root=json.optJSONArray(kind) ?: return listOf(fallback)
        val a=root.optJSONArray(i) ?: return emptyList()
        return (0 until a.length()).map { a.getString(it) }
    }
    fun key(name:String)=keyboard.indexOfFirst { a -> a.any { InputBindings.canonical(it).equals(InputBindings.canonical(name),true) } }.takeIf { it>=0 }
    fun pad(name:String)=controller.indexOfFirst { name in it }.takeIf { it>=0 }
    fun velocity(s:FloatArray,turns:Int):Pair<Int,Int> {
        if(!enabled) return 0 to 0
        var x=(s[3]-s[2]).coerceIn(-1f,1f); var y=(s[1]-s[0]).coerceIn(-1f,1f)
        val v=displayVectorToGuest(x,y,turns)
        x=v.first;y=v.second;val scale=speed/hypot(x,y).coerceAtLeast(1f)
        return kotlin.math.round(x*scale).toInt() to kotlin.math.round(y*scale).toInt()
    }
    companion object { val directions=arrayOf("Up","Down","Left","Right") }
}
