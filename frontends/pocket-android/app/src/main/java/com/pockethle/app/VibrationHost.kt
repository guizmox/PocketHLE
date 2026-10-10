package com.pockethle.app

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager

/** One motor per emulated device, not per VIB1 file descriptor. */
object VibrationHost {
    private val sessions = mutableMapOf<Int, LongArray>()
    private var nextId = 1
    private var vibrator: Vibrator? = null
    private var foreground = false
    private var closing = false
    private var active = 0
    private const val MODERATE_AMPLITUDE = 128

    @Synchronized fun initialize(context: Context) {
        val app = context.applicationContext
        vibrator = if (Build.VERSION.SDK_INT >= 31)
            app.getSystemService(VibratorManager::class.java)?.defaultVibrator
        else app.getSystemService(Vibrator::class.java)
        closing = false
    }
    @Synchronized fun open(): Int {
        if (closing) return -995
        if (vibrator?.hasVibrator() != true) return -50
        if (nextId <= 0 || sessions.size >= 64) return -8
        val id = nextId++; sessions[id] = LongArray(0); return id
    }
    @Synchronized fun apply(id: Int, timings: LongArray): Int {
        if (closing) return 995
        if (!sessions.containsKey(id)) return 6
        if (timings.size > 2050 || timings.any { it < 0 } ||
            (timings.isNotEmpty() && (timings.size % 2 != 0 || timings.none { it > 0 }))) return 87
        val v = vibrator ?: return 50
        return try {
            if (foreground) play(v, timings)
            sessions[id] = timings.copyOf(); active = id; 0
        } catch (_: SecurityException) { 5 }
          catch (_: IllegalArgumentException) { 87 }
          catch (_: RuntimeException) { 21 }
    }
    private fun play(v: Vibrator, timings: LongArray) {
        if (timings.isEmpty()) { v.cancel(); return }
        if (Build.VERSION.SDK_INT >= 26) {
            if (v.hasAmplitudeControl()) {
                val amplitudes = IntArray(timings.size) { if (it % 2 == 0) 0 else MODERATE_AMPLITUDE }
                v.vibrate(VibrationEffect.createWaveform(timings, amplitudes, 0))
            } else v.vibrate(VibrationEffect.createWaveform(timings, 0))
        } else {
            @Suppress("DEPRECATION")
            v.vibrate(timings, 0)
        }
    }
    @Synchronized fun isGameVibrating(): Boolean = foreground && !closing && sessions[active]?.isNotEmpty() == true
    @Synchronized fun close(id: Int) {
        sessions.remove(id)
        if (active == id) { active = 0; runCatching { vibrator?.cancel() } }
    }
    @Synchronized fun pause() { foreground = false; runCatching { vibrator?.cancel() } }
    @Synchronized fun resume() {
        foreground = true
        if (!closing) sessions[active]?.let { timings -> vibrator?.let { v -> runCatching { play(v, timings) } } }
    }
    @Synchronized fun closeAll() {
        closing = true; foreground = false; active = 0; sessions.clear()
        runCatching { vibrator?.cancel() }
    }
}
