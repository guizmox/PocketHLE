package com.pockethle.app

import android.content.Context
import android.graphics.*
import android.util.AttributeSet
import android.view.View

/** Neutral SD card, matching the desktop library; no interpreted game artwork. */
class SdCardTile(context: Context, attrs: AttributeSet? = null): View(context,attrs) {
    private val paint=Paint(Paint.ANTI_ALIAS_FLAG)
    override fun onDraw(c: Canvas) {
        val w=width.toFloat();val h=height.toFloat(); val pad=resources.displayMetrics.density*5
        val shape=Path().apply { moveTo(pad+w*.17f,pad);lineTo(w-pad,pad);lineTo(w-pad,h-pad);lineTo(pad,h-pad);lineTo(pad,pad+h*.15f);close() }
        paint.color=0xff202c3e.toInt();paint.style=Paint.Style.FILL;c.drawPath(shape,paint)
        paint.color=0xff506179.toInt();paint.style=Paint.Style.STROKE;paint.strokeWidth=2*resources.displayMetrics.density;c.drawPath(shape,paint)
        paint.style=Paint.Style.FILL;paint.color=0xffcbb270.toInt()
        for(i in 0..7) c.drawRoundRect(w*(.25f+i*.08f),h*.055f,w*(.30f+i*.08f),h*.18f,2f,2f,paint)
        paint.color=0xff8997ad.toInt();paint.textSize=12*resources.displayMetrics.density;c.drawText("SD",w*.13f,h*.91f,paint)
    }
}
