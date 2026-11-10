package com.pockethle.app

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.TextView
import androidx.appcompat.widget.AppCompatImageView

/** Render the actual game layouts offscreen, at device size. No emulator/session exists. */
internal class ImportControlsPreview(context: Context) : AppCompatImageView(context) {
    init { scaleType = ScaleType.FIT_CENTER; isFocusable = false }

    fun update(gizmondo: Boolean, portrait: Boolean, config: LauncherConfig) {
        post {
            if (!isAttachedToWindow || width <= 0 || height <= 0) return@post
            val metrics = resources.displayMetrics
            val short = minOf(metrics.widthPixels, metrics.heightPixels)
            val long = maxOf(metrics.widthPixels, metrics.heightPixels)
            val w = if (portrait && !gizmondo) short else long
            val h = if (portrait && !gizmondo) long else short
            val game = FrameLayout(context).apply {
                id = if (gizmondo) R.id.game_area else R.id.content_column
                setBackgroundColor(0xff233547.toInt())
                addView(TextView(context).apply {
                    text = "Game screen"; gravity = Gravity.CENTER
                    setTextColor(Color.WHITE); textSize = 16f
                }, FrameLayout.LayoutParams(-1, -1))
            }
            val root: View = if (gizmondo) {
                GizmondoLayout(context).apply {
                    setBackgroundColor(0xff080d15.toInt())
                    addView(game, FrameLayout.LayoutParams(-1, -1))
                    addView(GizmondoControls(context).apply {
                        id = R.id.game_controls
                        bigDirectionalControls = config.gizmondoControlsSize == "big"
                        maximizeScreen = config.gizmondoMaximizeScreen
                    }, FrameLayout.LayoutParams(-1, -1))
                }
            } else {
                PocketPcLayout(context).apply {
                    addView(game)
                    addView(PocketPcControls(context).apply { id = R.id.game_controls; configure(config) })
                    setDisplaySize(if (portrait) 240 else 320, if (portrait) 320 else 240)
                }
            }
            root.measure(View.MeasureSpec.makeMeasureSpec(w, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(h, View.MeasureSpec.EXACTLY))
            root.layout(0, 0, w, h)
            val scale = minOf(width.toFloat() / w, height.toFloat() / h)
            val bitmap = Bitmap.createBitmap((w * scale).toInt().coerceAtLeast(1),
                (h * scale).toInt().coerceAtLeast(1), Bitmap.Config.ARGB_8888)
            root.draw(Canvas(bitmap).apply { scale(scale, scale) })
            setImageBitmap(bitmap)
            contentDescription = "${if (gizmondo) "Gizmondo" else "PocketPC"} ${if (portrait) "portrait" else "landscape"} controls preview"
        }
    }
}
