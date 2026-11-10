package com.pockethle.app

import android.content.Context
import android.graphics.Typeface
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.appcompat.app.AlertDialog
import org.json.JSONObject

/** First-import choices; only Save or Keep current settings completes the flow. */
internal class ImportSetupDialog(
    private val context: Context,
    val platform: String,
    private val base: LauncherConfig,
    private val hostKey: (KeyEvent) -> Boolean,
    restored: JSONObject?,
    private val finish: (JSONObject) -> Unit,
) {
    private val gizmondo = platform == "gizmondo"
    private var step = restored?.optInt("step", 0)?.coerceIn(0, if (gizmondo) 2 else 0) ?: 0
    private val initialSupersampling = base.gpuRenderer && base.glesInternalResolution > 1
    private var supersampling = restored?.optBoolean("supersampling", initialSupersampling) ?: initialSupersampling
    private var camera = restored?.optBoolean("camera", base.cameraEnabled) ?: base.cameraEnabled
    private var bluetooth = restored?.optBoolean("bluetooth", base.bluetoothEnabled) ?: base.bluetoothEnabled
    private var location = restored?.optBoolean("location", base.gpsEnabled) ?: base.gpsEnabled
    private var data = restored?.optBoolean("data", base.gprsEnabled) ?: base.gprsEnabled
    private var size = restored?.optString("size")?.takeIf { it in listOf("normal", "big") }
        ?: if (gizmondo) base.gizmondoControlsSize else base.pocketpcControlsSize
    private var layout = restored?.optString("layout")?.takeIf { it in listOf("classic", "controller") } ?: base.pocketpcTouchLayout
    private var overlap = restored?.optBoolean("overlap", base.gizmondoMaximizeScreen) ?: base.gizmondoMaximizeScreen
    private var portrait = restored?.optBoolean("portrait", true) ?: true
    private val dialog = AddDataDialog(context, hostKey)
    private val content = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        val pad = (16 * resources.displayMetrics.density).toInt(); setPadding(pad, pad, pad, pad)
    }
    private val scroll = ScrollView(context).apply { addView(content) }
    private var preview: ImportControlsPreview? = null
    private var status: TextView? = null
    private var busy = false

    fun snapshot(): JSONObject = JSONObject().put("platform", platform).put("step", step)
        .put("supersampling", supersampling).put("camera", camera).put("bluetooth", bluetooth)
        .put("location", location).put("data", data).put("size", size).put("layout", layout)
        .put("overlap", overlap).put("portrait", portrait)

    fun show() {
        dialog.setTitle(if (gizmondo) "Welcome to Gizmondo" else "PocketPC touch controls")
        dialog.setView(scroll)
        dialog.setButton(AlertDialog.BUTTON_NEGATIVE, "Keep current settings") { _, _ -> }
        dialog.setButton(AlertDialog.BUTTON_NEUTRAL, "Back") { _, _ -> }
        dialog.setButton(AlertDialog.BUTTON_POSITIVE, "Next") { _, _ -> }
        dialog.setCanceledOnTouchOutside(false)
        dialog.setOnCancelListener { requestFinish(true) }
        dialog.show()
        val metrics = context.resources.displayMetrics
        dialog.window?.setLayout((metrics.widthPixels * .94f).toInt(), (metrics.heightPixels * .90f).toInt())
        dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener { requestFinish(true) }
        dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener { step--; rebuild() }
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            if (gizmondo && step < 2) { step++; rebuild() } else requestFinish(false)
        }
        rebuild()
    }

    private fun text(value: String, title: Boolean = false) {
        content.addView(TextView(context).apply {
            text = value; textSize = if (title) 18f else 14f
            if (title) setTypeface(typeface, Typeface.BOLD)
            setPadding(0, 4, 0, 12)
        })
    }
    private fun checkbox(label: String, selected: Boolean, changed: (Boolean) -> Unit) {
        content.addView(CheckBox(context).apply {
            text = label; isChecked = selected
            setOnCheckedChangeListener { _, checked -> changed(checked) }
        })
    }
    private fun choices(labels: List<String>, values: List<String>, selected: String, changed: (String) -> Unit) {
        content.addView(RadioGroup(context).apply {
            orientation = RadioGroup.HORIZONTAL
            labels.forEachIndexed { i, label ->
                addView(RadioButton(context).apply { id = View.generateViewId(); text = label; tag = values[i]; isChecked = values[i] == selected },
                    RadioGroup.LayoutParams(0, -2, 1f))
            }
            setOnCheckedChangeListener { group, id ->
                group.findViewById<RadioButton>(id)?.tag?.let { changed(it.toString()) }
            }
        })
    }
    private fun previewConfig() = base.copy(
        pocketpcTouchLayout = layout, pocketpcControlsSize = size,
        gizmondoControlsSize = size, gizmondoMaximizeScreen = overlap,
    )
    private fun refreshPreview() { preview?.update(gizmondo, portrait, previewConfig()) }
    private fun rebuild() {
        content.removeAllViews(); preview = null
        when {
            gizmondo && step == 0 -> {
                text("Sharper graphics", true)
                checkbox("Enable supersampling (2x to start)", supersampling) { supersampling = it }
                text("Renders 3D games at a higher internal resolution for a cleaner image. Requires GPU rendering and more GPU power; it adds no detail to 2D artwork.")
                text("This is the global OpenGL ES setting and also affects PocketPC GPU games. An existing higher scale is kept. Per-game renderer overrides still apply.")
            }
            gizmondo && step == 1 -> {
                text("Hardware connections", true)
                text("Let games use the corresponding host services. Android permissions still apply.")
                checkbox("Camera", camera) { camera = it }
                checkbox("Bluetooth", bluetooth) { bluetooth = it }
                checkbox("Location / GPS", location) { location = it }
                checkbox("Internet / data (host connection)", data) { data = it }
            }
            else -> {
                text(if (gizmondo) "Touch controls" else "Choose your touch controls", true)
                if (!gizmondo) {
                    text("Layout")
                    choices(listOf("Classic", "Controller"), listOf("classic", "controller"), layout) { layout = it; refreshPreview() }
                }
                text(if (gizmondo) "D-pad size" else "D-pad and stylus stick size")
                choices(listOf("Normal", "Big"), listOf("normal", "big"), size) { size = it; refreshPreview() }
                text("Big controls take up more space and can reduce the game image size.")
                if (gizmondo) {
                    checkbox("Allow controls over the game image", overlap) { overlap = it; refreshPreview() }
                    text("Allow overlap to keep a larger game image, including with Big controls. Controls may cover part of the picture. Leave off to keep them separate.")
                } else {
                    text("Preview orientation")
                    choices(listOf("Portrait", "Landscape"), listOf("portrait", "landscape"), if (portrait) "portrait" else "landscape") {
                        portrait = it == "portrait"; refreshPreview()
                    }
                }
                text("Live layout preview — no game is running. The image adapts to the space left by the controls.")
                preview = ImportControlsPreview(context).also {
                    content.addView(it, LinearLayout.LayoutParams(-1, (context.resources.displayMetrics.heightPixels * .40f).toInt()))
                }
                refreshPreview()
            }
        }
        status = TextView(context).also { content.addView(it) }
        dialog.getButton(AlertDialog.BUTTON_NEUTRAL).visibility = if (step > 0) View.VISIBLE else View.GONE
        dialog.getButton(AlertDialog.BUTTON_POSITIVE).text = if (gizmondo && step < 2) "Next" else "Save choices"
        scroll.scrollTo(0, 0)
    }
    private fun requestFinish(keep: Boolean) {
        if (busy) return
        val value = JSONObject().put("platform", platform)
        if (!keep) {
            if (gizmondo) {
                if (supersampling != initialSupersampling) value.put("supersampling", supersampling)
                value.put("camera", camera).put("bluetooth", bluetooth).put("location", location).put("data", data)
                    .put("controls_size", size).put("overlap", overlap)
            } else value.put("layout", layout).put("controls_size", size)
        }
        setBusy(true)
        finish(value)
    }
    private fun setBusy(value: Boolean) {
        busy = value
        dialog.controllerCancel = !value
        dialog.setCancelable(!value)
        fun enable(view: View) {
            view.isEnabled = !value
            if (view is ViewGroup) for (i in 0 until view.childCount) enable(view.getChildAt(i))
        }
        enable(content)
        for (button in listOf(AlertDialog.BUTTON_POSITIVE, AlertDialog.BUTTON_NEGATIVE, AlertDialog.BUTTON_NEUTRAL)) {
            dialog.getButton(button)?.isEnabled = !value
        }
    }
    fun failed(message: String) {
        if (!dialog.isShowing) dialog.show()
        setBusy(false)
        status?.text = "Could not save choices: $message"
    }
    fun dispose() { dialog.setOnCancelListener(null); dialog.dismiss(); preview?.setImageDrawable(null) }
}
