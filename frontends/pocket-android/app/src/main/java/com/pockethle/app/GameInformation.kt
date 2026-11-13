package com.pockethle.app

import android.content.Context
import android.graphics.Typeface
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import org.json.JSONObject

/** The same sections and detected values as the desktop binary information panel. */
object GameInformation {
    fun show(context: Context, title: String, metadata: JSONObject) {
        val density = context.resources.displayMetrics.density
        fun dp(value: Int) = (value * density).toInt()
        val content = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), dp(16))
        }
        fun text(value: String, heading: Boolean = false) {
            content.addView(TextView(context).apply {
                this.text = value
                textSize = if (heading) 14f else 15f
                if (heading) { setTypeface(typeface, Typeface.BOLD); setPadding(0, dp(18), 0, dp(6)) }
                else { setPadding(0, 0, 0, dp(8)); setTextIsSelectable(true) }
            })
        }
        text(metadata.optString("platform"))
        metadata.optString("title_id").takeIf { !metadata.isNull("title_id") && it.isNotBlank() }
            ?.let { text("Title ID: $it") }
        for ((key, heading) in listOf(
            "identity" to "IDENTITY", "executable" to "EXECUTABLE",
            "dependencies" to "DEPENDENCIES",
            "graphics" to "DISPLAY & GRAPHICS", "hardware" to "HARDWARE",
            "game_data" to "GAME DATA", "languages" to "LANGUAGES"
        )) {
            val rows = metadata.optJSONArray(key) ?: continue
            if (rows.length() == 0) continue
            text(heading, true)
            for (i in 0 until rows.length()) {
                val pair = rows.optJSONArray(i) ?: continue
                text("${pair.optString(0)}\n${pair.optString(1)}")
            }
        }
        val evidence = metadata.optJSONArray("evidence")
        if (evidence != null && evidence.length() > 0) {
            text("TECHNICAL DETAILS / DETECTION EVIDENCE", true)
            for (i in 0 until evidence.length()) text(evidence.optString(i))
        }
        AlertDialog.Builder(context).setTitle(title)
            .setView(ScrollView(context).apply { addView(content) })
            .setPositiveButton(android.R.string.ok, null).show()
    }
}
