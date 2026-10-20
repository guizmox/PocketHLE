package com.pockethle.app

import android.graphics.*
import android.graphics.drawable.Drawable

/** Shared Pocket PC tile: keep the icon's aspect ratio inside the device screen. */
class PocketPcArtworkDrawable(private val icon: Bitmap?) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
    override fun draw(canvas: Canvas) {
        val save = canvas.save()
        val scale = minOf(bounds.width() / 126f, bounds.height() / 126f)
        canvas.translate(bounds.exactCenterX() - 63f * scale, bounds.exactCenterY() - 63f * scale)
        canvas.scale(scale, scale)
        paint.color = Color.rgb(98,112,130)
        paint.style = Paint.Style.FILL
        canvas.drawRoundRect(22f,5f,104f,121f,10f,10f,paint)
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 1.5f
        paint.color = Color.rgb(158,174,193)
        canvas.drawRoundRect(22f,5f,104f,121f,10f,10f,paint)
        paint.style = Paint.Style.FILL
        paint.color = Color.rgb(38,61,80)
        canvas.drawRoundRect(30f,16f,96f,97f,3f,3f,paint)
        if (icon != null) {
            val factor = minOf(56f / icon.width, 71f / icon.height)
            val w = icon.width * factor
            val h = icon.height * factor
            canvas.drawBitmap(icon,null,RectF(63f-w/2,56.5f-h/2,63f+w/2,56.5f+h/2),paint)
        } else {
            paint.color = Color.rgb(198,220,234)
            paint.textSize = 12f
            paint.textAlign = Paint.Align.CENTER
            canvas.drawText("Pocket PC",63f,60f,paint)
        }
        paint.color = Color.rgb(204,214,225)
        canvas.drawCircle(63f,109f,5f,paint)
        canvas.restoreToCount(save)
    }
    override fun setAlpha(alpha: Int) { paint.alpha = alpha; invalidateSelf() }
    override fun setColorFilter(colorFilter: ColorFilter?) { paint.colorFilter = colorFilter; invalidateSelf() }
    @Deprecated("Drawable opacity")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT
}
