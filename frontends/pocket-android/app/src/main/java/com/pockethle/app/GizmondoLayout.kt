package com.pockethle.app

import android.content.Context
import android.util.AttributeSet
import android.view.View
import android.widget.FrameLayout

/** Reserve directional-control space before measuring the game, including Big. */
class GizmondoLayout(context: Context, attrs: AttributeSet? = null) : FrameLayout(context, attrs) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val controls = findViewById<GizmondoControls>(R.id.game_controls)
        val area = findViewById<View>(R.id.game_area)
        if (area != null) {
            val w = (MeasureSpec.getSize(widthMeasureSpec) - paddingLeft - paddingRight).coerceAtLeast(1)
            val h = (MeasureSpec.getSize(heightMeasureSpec) - paddingTop - paddingBottom).coerceAtLeast(1)
            val room = GizmondoGeometry.calculate(w, h, resources.displayMetrics.density,
                controls?.bigDirectionalControls == true, controls?.maximizeScreen == true).screen
            val params = area.layoutParams as LayoutParams
            params.leftMargin = room.x; params.rightMargin = w - room.x - room.w
            params.topMargin = room.y; params.bottomMargin = h - room.y - room.h
        }
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
    }
}
