package com.pockethle.app

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager

/** Sticky broadcast snapshot: no receiver registered, no permission, no Activity retained. */
object BatteryHost {
    @Volatile private var context: Context? = null
    fun initialize(ctx: Context) { context = ctx.applicationContext }
    private fun pack(ac: Int, flags: Int, percent: Int) = ac or (flags shl 8) or (percent shl 16)
    @JvmStatic fun read(): Int {
        val unknown = pack(255, 255, 255)
        return try {
            val battery = context?.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
                ?: return unknown
            val ac = if (!battery.hasExtra(BatteryManager.EXTRA_PLUGGED)) 255
                else if (battery.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) == 0) 0 else 1
            if (!battery.hasExtra(BatteryManager.EXTRA_PRESENT)) return unknown
            if (!battery.getBooleanExtra(BatteryManager.EXTRA_PRESENT, true)) return pack(ac, 128, 255)
            val level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
            val percent = if (level >= 0 && scale > 0)
                ((level.toLong() * 100 / scale).coerceIn(0, 100)).toInt() else 255
            val charging = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1) == BatteryManager.BATTERY_STATUS_CHARGING
            val flags = if (percent == 255) 255 else
                (if (percent <= 5) 4 else if (percent <= 33) 2 else if (percent > 66) 1 else 0) or
                    (if (charging) 8 else 0)
            pack(ac, flags, percent)
        } catch (_: RuntimeException) { unknown }
    }
}
