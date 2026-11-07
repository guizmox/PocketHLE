package com.pockethle.app

import android.content.Intent
import android.os.Bundle
import android.view.InputDevice
import android.view.KeyEvent
import android.view.View
import android.view.Window
import android.view.WindowManager
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import org.json.JSONObject

/** Reserved physical host keys are handled before launcher widgets or capture dialogs. */
abstract class HostKeyActivity : AppCompatActivity() {
    private val hostController = DeckHostTracker()

    protected open fun hostGizmondo() = false

    private var hostFullscreen = false
    private val normalWindows = java.util.WeakHashMap<Window, Pair<Int, Int>>()
    private var barWasShowing = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        hostFullscreen = savedInstanceState?.getBoolean("host_fullscreen") ?: false
    }

    override fun onSaveInstanceState(out: Bundle) {
        super.onSaveInstanceState(out)
        out.putBoolean("host_fullscreen", hostFullscreen)
    }

    override fun onResume() {
        super.onResume()
        if (hostFullscreen && supportActionBar?.isShowing == true) {
            barWasShowing = true
            supportActionBar?.hide()
        }
        applyHostFullscreen(window)
    }

    protected fun applyHostFullscreen(target: Window) {
        if (hostFullscreen) {
            normalWindows.putIfAbsent(
                target,
                target.attributes.flags to target.decorView.systemUiVisibility,
            )
            target.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
            target.decorView.systemUiVisibility =
                target.decorView.systemUiVisibility or
                    View.SYSTEM_UI_FLAG_FULLSCREEN or
                    View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
        } else
            normalWindows.remove(target)?.let { (flags, ui) ->
                target.setFlags(flags, WindowManager.LayoutParams.FLAG_FULLSCREEN)
                target.decorView.systemUiVisibility = ui
            }
    }

    protected open fun onHostFullscreenChanged() {}

    protected open fun onHostFilterChanged(filter: String) {
        recreate()
    }

    protected fun handleHostKeyEvent(event: KeyEvent): Boolean {
        val source = event.device?.sources ?: event.source
        val pad =
            source and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
                source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
        if (pad) {
            val control = InputBindings.controllerName(event.keyCode) ?: return false
            if (!DeckInputPolicy.reserved(control, hostGizmondo())) return false
            if (event.action == KeyEvent.ACTION_DOWN) {
                hostController.down(event.deviceId, control, event.eventTime, hostGizmondo())
            } else if (event.action == KeyEvent.ACTION_UP) {
                hostController.up(event.deviceId, control, event.eventTime, hostGizmondo())?.let {
                    runHostAction(it)
                }
            }
            return true
        }
        val name = InputBindings.keyName(event) ?: return false
        if (!DeckInputPolicy.reservedKey(name)) return false
        if (event.action != KeyEvent.ACTION_DOWN || event.repeatCount != 0) return true
        DeckInputPolicy.key(name)?.let { runHostAction(it) }
        return true
    }

    private fun runHostAction(action: DeckInputPolicy.Action) {
        when (action) {
            DeckInputPolicy.Action.ROTATION -> {} // Requires an active game image.
            DeckInputPolicy.Action.EXIT -> {
                startActivity(
                    Intent(this, MainActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                )
                if (this !is MainActivity) finish()
            }
            DeckInputPolicy.Action.NEXT_FILTER -> {
                val result =
                    runCatching {
                            val root = LibraryPaths.root(this)
                            val json = JSONObject(NativeBridge.readConfig(root))
                            check(!json.has("ok") || json.optBoolean("ok", true)) {
                                json.optString("error")
                            }
                            val filters = DeckInputPolicy.filters
                            val index =
                                filters.indexOf(json.optString("upscale_filter", "reconstruction"))
                            val next = filters[(index + 1).mod(filters.size)]
                            json.put("upscale_filter", next)
                            val saved = JSONObject(NativeBridge.writeConfig(root, json.toString()))
                            check(saved.optBoolean("ok", false)) { saved.optString("error") }
                            onHostFilterChanged(next)
                            "Filter: $next"
                        }
                        .getOrElse { it.message ?: "Could not change filter" }
                Toast.makeText(this, result, Toast.LENGTH_SHORT).show()
            }
            DeckInputPolicy.Action.FULLSCREEN -> {
                if (!hostFullscreen) barWasShowing = supportActionBar?.isShowing == true
                hostFullscreen = !hostFullscreen
                if (hostFullscreen) supportActionBar?.hide()
                else if (barWasShowing) supportActionBar?.show()
                applyHostFullscreen(window)
                onHostFullscreenChanged()
            }
        }
    }

    override fun onPause() {
        hostController.clear()
        super.onPause()
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean =
        if (handleHostKeyEvent(event)) true else super.dispatchKeyEvent(event)
}
