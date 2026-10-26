package com.pockethle.app

private fun overlap(a: ControlRect,b: ControlRect)=a.x<b.x+b.w && b.x<a.x+a.w && a.y<b.y+b.h && b.y<a.y+a.h
fun main() {
    var layouts=0
    var comparisons=0
    for(density in listOf(1f,2f,2.625f,3f)) {
        for(size in listOf(360 to 800,412 to 915,480 to 960,800 to 1280,800 to 360,915 to 412,1280 to 800)) {
            val w=(size.first*density).toInt();val h=(size.second*density).toInt()
            val g=PocketPcGeometry.calculate(w,h,density)
            val zones=g.actions+g.auxiliary+listOf(g.dpad,g.stick,g.screen)
            for(r in zones) check(r.x>=0 && r.y>=0 && r.w>0 && r.h>0 && r.x+r.w<=w && r.y+r.h<=h) { "bounds $size/$density $r" }
            for(i in zones.indices) for(j in i+1 until zones.size) {
                check(!overlap(zones[i],zones[j])) { "overlap $size/$density $i/$j" };comparisons++
            }
            for(r in g.actions) check(r.y+r.h+16*density<=g.dpad.y) { "thumb clearance" }
            if(w<h) {
                val landscape=PocketPcGeometry.calculate(h,w,density)
                check(g.dpad.w==landscape.dpad.w && g.stick.w==landscape.stick.w)
                val gap=(6*density).toInt().coerceAtLeast(1)
                val oldPad=minOf((112*density).toInt(),(w-6*gap)/3).coerceAtLeast(1)
                check(g.dpad.w>oldPad) { "portrait controls must grow" }
                val rowH=minOf((44*density).toInt().coerceAtLeast(1),h/14).coerceAtLeast(1)
                val oldActionY=h-gap-maxOf(oldPad,(108*density).toInt())-(18*density).toInt()-2*rowH-gap
                check(g.actions.minOf { it.y }<oldActionY) { "buttons must move upward" }
                for(native in listOf(240 to 320,480 to 640)) {
                    val fit=pocketPcDisplayRect(g.screen.w,g.screen.h,native.first,native.second,4)
                    val point=displayPointToGuest(fit.left+fit.width/2f,fit.top+fit.height/2f,g.screen.w,g.screen.h,native.first,native.second,0,4,true)
                    check(point!=null && kotlin.math.abs(point.first-native.first/2)<=1 && kotlin.math.abs(point.second-native.second/2)<=1)
                }
            }
            layouts++
        }
    }
    println("PASS: $layouts real Kotlin layouts, $comparisons pairwise overlap checks, portrait/landscape size parity, raised action rows and QVGA/VGA centre input mapping.")
}
